// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ProjectSyncActor — three-phase project structure synchronisation.
//!
//! Keeps the knowledge graph in sync with the actual filesystem state.
//! Uses the `analyzer` module for semantic classification (roles, languages,
//! build systems) so that every graph node is enriched with full metadata.
//!
//! # Three Sync Phases
//!
//! 1. **Bootstrap** — Cold start: no Project node exists. Full scan + create-all.
//! 2. **Startup sync** — Warm start: Project node exists. Content-hash manifest diff.
//! 3. **Continuous sync** — Real-time: file change events from VS Code watcher.
//!
//! # Semantic Enrichment
//!
//! During bootstrap, the actor uses the `analyzer` module to:
//! - Classify every file and directory with a semantic role
//! - Detect programming languages
//! - Identify entry points (main.rs, main.py, etc.)
//! - Parse build config files into structured `BuildMetadata`
//! - Create `BuildSystem` nodes in the graph for each detected build system
//!
//! This eliminates the need for a separate `ProjectAnalyzerActor` scan —
//! the analysis summary can be derived from graph queries.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

use crate::actors::Actor;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::analyzer::build_parsers;
use crate::analyzer::scanner as analyzer_scanner;
use crate::models::embedding::Embedder;
use crate::models::memory_graph::{
    GraphEdge, GraphNode, NodeFilter, NodeInput, NodeType, NodeUpdate, RelationshipInput,
    RelationshipType,
};

// ============================================================================
// Constants
// ============================================================================

/// Config key for the file manifest hash (SHA-256 of sorted "path|size\n" lines).
const CONFIG_FILE_MANIFEST_HASH: &str = "project.file_manifest_hash";

/// Config key for the build manifest hash (SHA-256 of build config file contents).
const CONFIG_BUILD_MANIFEST_HASH: &str = "project.build_manifest_hash";

/// Config key for the last sync timestamp.
const CONFIG_LAST_SYNCED_AT: &str = "project.last_synced_at";

/// Config key for the project root path.
const CONFIG_PROJECT_ROOT: &str = "project.root_path";

/// Debounce window for file change events (milliseconds).
const FILE_EVENT_DEBOUNCE_MS: u64 = 500;

/// Known build config file names.
const BUILD_CONFIG_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "pnpm-workspace.yaml",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "go.mod",
    "build.gradle",
    "build.gradle.kts",
    "pom.xml",
    "CMakeLists.txt",
    "Makefile",
    "Gemfile",
    "Package.swift",
];

/// Directories to always skip.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".pnpm",
    "target",
    "dist",
    "build",
    "out",
    ".git",
    ".svn",
    ".hg",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".eggs",
    "eggs",
    ".spire",
];

// ============================================================================
// Change Type
// ============================================================================

/// The type of file system change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeType {
    Created,
    Modified,
    Deleted,
}

// ============================================================================
// Sync Result
// ============================================================================

/// Result of a sync operation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SyncResult {
    pub nodes_created: usize,
    pub nodes_updated: usize,
    pub nodes_deleted: usize,
    pub edges_created: usize,
    pub edges_deleted: usize,
    pub embeddings_generated: usize,
    pub duration_ms: u64,
}

impl SyncResult {
    fn new() -> Self {
        Self {
            nodes_created: 0,
            nodes_updated: 0,
            nodes_deleted: 0,
            edges_created: 0,
            edges_deleted: 0,
            embeddings_generated: 0,
            duration_ms: 0,
        }
    }
}

// ============================================================================
// File Manifest
// ============================================================================

/// A lightweight file manifest entry (name + size, no content).
#[derive(Debug, Clone)]
struct ManifestEntry {
    path: String,
    size: u64,
    modified: Option<DateTime<Utc>>,
}

/// Compute a SHA-256 hash of a file manifest.
/// The manifest is sorted by path, then each line is "path|size\n".
fn hash_manifest(entries: &[ManifestEntry]) -> String {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let mut hasher = Sha256::new();
    for entry in &sorted {
        hasher.update(format!("{}|{}\n", entry.path, entry.size));
    }
    format!("{:x}", hasher.finalize())
}

// ============================================================================
// ProjectSyncMessage
// ============================================================================

/// Messages for the ProjectSync actor.
pub enum ProjectSyncMessage {
    /// Initialize the actor with its dependencies.
    Initialize {
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        embedder: Arc<dyn Embedder>,
        reply_to: oneshot::Sender<Result<()>>,
    },
    /// Full bootstrap scan — create the entire project tree in the graph.
    Bootstrap {
        project_root: PathBuf,
        reply_to: oneshot::Sender<Result<SyncResult>>,
    },
    /// Quick startup verification — content-hash manifest diff.
    StartupSync {
        project_root: PathBuf,
        reply_to: oneshot::Sender<Result<SyncResult>>,
    },
    /// Incoming file change event from VS Code watcher.
    FileEvent {
        change_type: ChangeType,
        path: PathBuf,
        reply_to: oneshot::Sender<Result<SyncResult>>,
    },
    /// Force a full re-sync of the entire project.
    ForceResync {
        project_root: PathBuf,
        reply_to: oneshot::Sender<Result<SyncResult>>,
    },
}


// ============================================================================
// ProjectSyncActor
// ============================================================================

/// The ProjectSync actor — three-phase project structure synchronisation.
pub struct ProjectSyncActor {
    /// Sender to the MemoryGraph actor for all graph mutations.
    memory_graph_tx: Option<mpsc::Sender<MemoryGraphMessage>>,

    /// Embedder for generating vector embeddings for file/directory nodes.
    embedder: Option<Arc<dyn Embedder>>,

    /// Path to the project-analyzer binary (or empty to use library mode).
    analyzer_bin: Option<PathBuf>,
}

impl ProjectSyncActor {
    pub fn new() -> Self {
        Self {
            memory_graph_tx: None,
            embedder: None,
            analyzer_bin: None,
        }
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    /// Send a message to the MemoryGraph actor and await the response.
    async fn send_to_graph<T, F>(&self, make_msg: F) -> Result<T>
    where
        F: FnOnce(oneshot::Sender<Result<T>>) -> MemoryGraphMessage,
    {
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow!("MemoryGraph sender not initialized")
        })?;
        let (tx, rx) = oneshot::channel();
        tx_ref
            .send(make_msg(tx))
            .await
            .map_err(|e| anyhow!("MemoryGraph channel closed: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("MemoryGraph response error: {}", e))?
    }

    /// Store a config key-value pair in the graph.
    async fn set_config(&self, key: &str, value: serde_json::Value) -> Result<()> {
        self.send_to_graph(|tx| MemoryGraphMessage::SetConfig {
            key: key.to_string(),
            value,
            reply_to: tx,
        })
        .await
    }

    /// Get a config value from the graph.
    async fn get_config(&self, key: &str) -> Result<Option<serde_json::Value>> {
        self.send_to_graph(|tx| MemoryGraphMessage::GetConfig {
            key: key.to_string(),
            reply_to: tx,
        })
        .await
    }

    /// Create a node in the graph.
    async fn create_node(&self, input: NodeInput) -> Result<GraphNode> {
        self.send_to_graph(|tx| MemoryGraphMessage::StoreNode {
            node: input,
            reply_to: tx,
        })
        .await
    }

    /// Update a node in the graph.
    async fn update_node(&self, id: String, updates: NodeUpdate) -> Result<GraphNode> {
        self.send_to_graph(|tx| MemoryGraphMessage::UpdateNode {
            id,
            updates,
            reply_to: tx,
        })
        .await
    }

    /// Delete a node from the graph.
    async fn delete_node(&self, id: String) -> Result<()> {
        self.send_to_graph(|tx| MemoryGraphMessage::DeleteNode {
            id,
            reply_to: tx,
        })
        .await
    }

    /// Create a relationship in the graph.
    async fn create_relationship(&self, rel: RelationshipInput) -> Result<GraphEdge> {
        self.send_to_graph(|tx| MemoryGraphMessage::CreateRelationship {
            rel,
            reply_to: tx,
        })
        .await
    }

    /// Query nodes by filter.
    async fn query_nodes(&self, filter: NodeFilter) -> Result<Vec<GraphNode>> {
        self.send_to_graph(|tx| MemoryGraphMessage::QueryNodes {
            filter,
            reply_to: tx,
        })
        .await
    }

    /// Get a single node by ID.
    #[allow(dead_code)]
    async fn get_node(&self, id: &str) -> Result<Option<GraphNode>> {
        self.send_to_graph(|tx| MemoryGraphMessage::GetNode {
            id: id.to_string(),
            reply_to: tx,
        })
        .await
    }

    /// Get relationships for a node.
    #[allow(dead_code)]
    async fn get_relationships(&self, node_id: &str) -> Result<Vec<GraphEdge>> {
        self.send_to_graph(|tx| MemoryGraphMessage::GetRelationships {
            node_id: node_id.to_string(),
            reply_to: tx,
        })
        .await
    }

    /// Delete a relationship.
    #[allow(dead_code)]
    async fn delete_relationship(&self, id: String) -> Result<()> {
        self.send_to_graph(|tx| MemoryGraphMessage::DeleteRelationship {
            id,
            reply_to: tx,
        })
        .await
    }

    /// Generate an embedding for text and store it on a node.
    async fn generate_embedding(&self, node_id: &str, text: &str) -> Result<String> {
        let embedder_ref = self.embedder.as_ref().ok_or_else(|| {
            anyhow!("Embedder not initialized")
        })?;
        let _embedding = embedder_ref.embed(text).await?;
        let emb_id = format!("emb_{}", node_id);

        // Update the node with the embedding ID
        self.send_to_graph(|tx| MemoryGraphMessage::UpdateNode {
            id: node_id.to_string(),
            updates: NodeUpdate {
                node_type: None,
                subtype: None,
                name: None,
                description: None,
                properties: None,
                embedding_id: Some(Some(emb_id.clone())),
            },
            reply_to: tx,
        })
        .await?;

        Ok(emb_id)
    }

    /// Scan the filesystem and produce a file manifest.
    fn scan_manifest(root: &Path) -> Result<Vec<ManifestEntry>> {
        let mut entries = Vec::new();

        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                // Skip hidden files/dirs
                if name.starts_with('.') && name != "." {
                    return false;
                }
                // Skip known non-project dirs
                if e.file_type().is_dir() && SKIP_DIRS.contains(&name.as_ref()) {
                    return false;
                }
                true
            })
        {
            let entry = entry?;
            if !entry.file_type().is_file() {
                continue;
            }

            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let meta = entry.metadata()?;
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| {
                    let duration = t
                        .duration_since(std::time::UNIX_EPOCH)
                        .ok()?;
                    Some(
                        DateTime::from_timestamp(
                            duration.as_secs() as i64,
                            duration.subsec_nanos(),
                        )
                        .unwrap_or_default(),
                    )
                });

            entries.push(ManifestEntry {
                path: relative,
                size: meta.len(),
                modified,
            });
        }

        Ok(entries)
    }

    /// Check if a path is a build config file.
    fn is_build_config(path: &str) -> bool {
        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        BUILD_CONFIG_FILES.contains(&filename)
    }

    /// Build a structured description for a file node (used for embedding).
    fn build_file_description(
        path: &str,
        language: &str,
        role: &str,
        lines: usize,
        size: u64,
    ) -> String {
        serde_json::json!({
            "path": path,
            "language": language,
            "role": role,
            "lines": lines,
            "size": size,
        })
        .to_string()
    }

    /// Build a structured description for a directory node (used for embedding).
    fn build_directory_description(
        path: &str,
        role: &str,
        child_count: usize,
        languages: &[String],
    ) -> String {
        serde_json::json!({
            "path": path,
            "role": role,
            "child_count": child_count,
            "languages": languages,
        })
        .to_string()
    }

    /// Classify the role of a file based on its path and extension.
    fn classify_file_role(path: &str) -> &'static str {
        let path_lower = path.to_lowercase();
        let filename = Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");

        // Build config files
        if Self::is_build_config(path) {
            return "build_config";
        }

        // Entry points
        if filename == "main.rs"
            || filename == "main.ts"
            || filename == "main.py"
            || filename == "main.go"
            || filename == "index.ts"
            || filename == "index.js"
            || filename == "index.tsx"
            || filename == "lib.rs"
            || filename == "mod.rs"
        {
            return "entry_point";
        }

        // Test files
        if path_lower.contains("/test")
            || path_lower.contains("/tests")
            || path_lower.contains("_test.")
            || path_lower.contains(".spec.")
            || path_lower.contains(".test.")
            || filename.starts_with("test_")
        {
            return "test";
        }

        // Documentation
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if matches!(ext, "md" | "mdx" | "rst" | "txt" | "adoc") {
            return "documentation";
        }

        // Config files
        if matches!(
            ext,
            "json" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "env"
        ) {
            return "config";
        }

        // Source code
        if matches!(
            ext,
            "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "java"
                | "kt" | "kts" | "swift" | "c" | "cpp" | "h" | "hpp"
                | "css" | "scss" | "less" | "html" | "vue" | "svelte"
        ) {
            return "source";
        }

        "other"
    }

    /// Classify the role of a directory based on its name.
    fn classify_directory_role(name: &str) -> &'static str {
        match name {
            "src" | "lib" | "app" | "cmd" | "pkg" | "internal" => "source_code",
            "test" | "tests" | "spec" | "__tests__" | "testing" => "tests",
            "doc" | "docs" | "documentation" | "guide" | "guides" | "wiki" => "documentation",
            "script" | "scripts" | "bin" | "tool" | "tools" => "build_scripts",
            "config" | "configuration" | "cfg" | "settings" => "config",
            "example" | "examples" | "demo" | "demos" | "sample" | "samples" => "examples",
            "bench" | "benches" | "benchmark" | "benchmarks" => "benchmarks",
            "resource" | "resources" | "asset" | "assets" | "static" | "public" => "resources",
            "docker" | "ci" | ".github" | "deploy" | "deployment" => "deployment",
            "plugin" | "plugins" | "extension" | "extensions" | "addon" | "addons" => {
                "extensions"
            }
            "migration" | "migrations" | "db" | "database" | "schema" => "database",
            "i18n" | "locale" | "locales" | "lang" | "languages" | "translation"
            | "translations" => "localization",
            "target" | "dist" | "build" | "out" | "output" | "release" | "debug" => {
                "build_output"
            }
            "node_modules" | "vendor" | "third_party" | "third-party" | "deps"
            | "dependencies" => "dependencies",
            _ => "directory",
        }
    }

    /// Detect language from file extension.
    fn detect_language(extension: &str) -> &'static str {
        match extension {
            ".rs" => "Rust",
            ".ts" | ".tsx" => "TypeScript",
            ".js" | ".jsx" | ".mjs" | ".cjs" => "JavaScript",
            ".py" => "Python",
            ".go" => "Go",
            ".java" => "Java",
            ".kt" | ".kts" => "Kotlin",
            ".swift" => "Swift",
            ".c" | ".h" => "C",
            ".cpp" | ".hpp" | ".cc" | ".cxx" => "C++",
            ".cs" => "C#",
            ".rb" => "Ruby",
            ".php" => "PHP",
            ".scala" => "Scala",
            ".zig" => "Zig",
            ".md" | ".mdx" | ".rst" | ".adoc" => "Markdown",
            ".json" | ".yaml" | ".yml" | ".toml" => "Config",
            ".css" | ".scss" | ".less" => "CSS",
            ".html" | ".vue" | ".svelte" => "HTML",
            ".sql" => "SQL",
            ".sh" | ".bash" | ".zsh" => "Shell",
            ".gradle" | ".gradle.kts" => "Gradle",
            ".cmake" => "CMake",
            ".proto" => "Protobuf",
            ".dockerfile" | ".Dockerfile" => "Docker",
            _ => "Unknown",
        }
    }

    /// Estimate lines of code from file size (rough heuristic).
    fn estimate_lines(size: u64) -> usize {
        if size == 0 {
            return 0;
        }
        // Average ~50 bytes per line for source code
        std::cmp::max(1, (size / 50) as usize)
    }

    // ── Bootstrap ────────────────────────────────────────────────────────

    /// Phase 1: Full bootstrap scan.
    /// Creates the entire project tree in the graph from scratch.
    async fn bootstrap(&mut self, project_root: &Path) -> Result<SyncResult> {
        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        info!(
            "ProjectSync: bootstrapping project at {}",
            project_root.display()
        );

        // 1. Scan the filesystem
        let manifest = Self::scan_manifest(project_root)?;
        let manifest_hash = hash_manifest(&manifest);

        // 2. Create the Project node
        let project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let project_node = self
            .create_node(NodeInput {
                node_type: NodeType::Project,
                subtype: None,
                name: project_name.clone(),
                description: Some(format!("Project root: {}", project_root.display())),
                properties: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "path".to_string(),
                        serde_json::Value::String(project_root.to_string_lossy().to_string()),
                    );
                    m
                }),
                embedding_id: None,
            })
            .await?;
        result.nodes_created += 1;

        // 3. Build directory tree and create nodes
        let mut dir_entries: HashMap<String, Vec<String>> = HashMap::new();
        let mut file_entries: HashMap<String, ManifestEntry> = HashMap::new();
        let mut all_dirs: HashSet<String> = HashSet::new();

        // Collect all unique directories
        for entry in &manifest {
            let path = &entry.path;
            file_entries.insert(path.clone(), entry.clone());

            if let Some(parent) = Path::new(path).parent() {
                let parent_str = parent.to_string_lossy().to_string();
                if parent_str != "." && !parent_str.is_empty() {
                    all_dirs.insert(parent_str.clone());
                    dir_entries
                        .entry(parent_str.clone())
                        .or_default()
                        .push(path.clone());
                }
                if parent_str.is_empty() || parent_str == "." {
                    dir_entries
                        .entry(".".to_string())
                        .or_default()
                        .push(path.clone());
                }
            } else {
                dir_entries
                    .entry(".".to_string())
                    .or_default()
                    .push(path.clone());
            }
        }

        // Sort directories by depth descending (children first)
        let dirs_sorted: Vec<String> = {
            let mut d: Vec<String> = all_dirs.iter().cloned().collect();
            d.sort_by(|a, b| b.len().cmp(&a.len()));
            d
        };

        // Create directory nodes (bottom-up)
        let mut dir_node_ids: HashMap<String, String> = HashMap::new();

        for dir_path in &dirs_sorted {
            let dir_name = Path::new(dir_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(dir_path)
                .to_string();

            let role = Self::classify_directory_role(&dir_name);
            let child_count = dir_entries.get(dir_path).map(|v| v.len()).unwrap_or(0);

            let mut languages: Vec<String> = Vec::new();
            if let Some(children) = dir_entries.get(dir_path) {
                for child in children {
                    if let Some(entry) = file_entries.get(child) {
                        let ext = Path::new(&entry.path)
                            .extension()
                            .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                            .unwrap_or_default();
                        let lang = Self::detect_language(&ext).to_string();
                        if !languages.contains(&lang) {
                            languages.push(lang);
                        }
                    }
                }
            }

            let node = self
                .create_node(NodeInput {
                    node_type: NodeType::Unknown,
                    subtype: Some("Directory".to_string()),
                    name: dir_name,
                    description: Some(Self::build_directory_description(
                        dir_path, role, child_count, &languages,
                    )),
                    properties: Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "path".to_string(),
                            serde_json::Value::String(dir_path.clone()),
                        );
                        m.insert(
                            "role".to_string(),
                            serde_json::Value::String(role.to_string()),
                        );
                        m.insert(
                            "child_count".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(child_count)),
                        );
                        m
                    }),
                    embedding_id: None,
                })
                .await?;
            result.nodes_created += 1;
            dir_node_ids.insert(dir_path.clone(), node.id.clone());

            // Generate embedding for directory
            let desc =
                Self::build_directory_description(dir_path, role, child_count, &languages);
            if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                warn!("Failed to generate embedding for dir {}: {}", dir_path, e);
            } else {
                result.embeddings_generated += 1;
            }
        }

        // Create file nodes
        let mut file_node_ids: HashMap<String, String> = HashMap::new();

        for entry in &manifest {
            let path = &entry.path;
            let ext = Path::new(path)
                .extension()
                .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                .unwrap_or_default();
            let language = Self::detect_language(&ext).to_string();
            let role = Self::classify_file_role(path);
            let lines = Self::estimate_lines(entry.size);

            let filename = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let node = self
                .create_node(NodeInput {
                    node_type: NodeType::Unknown,
                    subtype: Some("File".to_string()),
                    name: filename,
                    description: Some(Self::build_file_description(
                        path, &language, role, lines, entry.size,
                    )),
                    properties: Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "path".to_string(),
                            serde_json::Value::String(path.clone()),
                        );
                        m.insert(
                            "extension".to_string(),
                            serde_json::Value::String(ext),
                        );
                        m.insert(
                            "language".to_string(),
                            serde_json::Value::String(language.clone()),
                        );
                        m.insert(
                            "role".to_string(),
                            serde_json::Value::String(role.to_string()),
                        );
                        m.insert(
                            "size".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(entry.size)),
                        );
                        m.insert(
                            "lines".to_string(),
                            serde_json::Value::Number(serde_json::Number::from(lines as u64)),
                        );
                        m
                    }),
                    embedding_id: None,
                })
                .await?;
            result.nodes_created += 1;
            file_node_ids.insert(path.clone(), node.id.clone());

            // Generate embedding for file
            let desc = Self::build_file_description(path, &language, role, lines, entry.size);
            if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                warn!("Failed to generate embedding for file {}: {}", path, e);
            } else {
                result.embeddings_generated += 1;
            }
        }

        // 4. Create Contains edges (directory → child)
        let root_dir_id = dir_node_ids
            .get(".")
            .cloned()
            .unwrap_or(project_node.id.clone());

        // Link project → root directory
        if root_dir_id != project_node.id {
            self.create_relationship(RelationshipInput {
                edge_type: RelationshipType::BelongsTo,
                from_id: root_dir_id.clone(),
                to_id: project_node.id.clone(),
                properties: None,
                weight: None,
            })
            .await?;
            result.edges_created += 1;
        }

        // Directory → child directories
        for dir_path in &dirs_sorted {
            let dir_id = match dir_node_ids.get(dir_path) {
                Some(id) => id.clone(),
                None => continue,
            };

            let parent_path = Path::new(dir_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| p != ".")
                .unwrap_or_else(|| ".".to_string());

            if let Some(parent_id) = dir_node_ids.get(&parent_path) {
                if parent_id != &dir_id {
                    self.create_relationship(RelationshipInput {
                        edge_type: RelationshipType::BelongsTo,
                        from_id: dir_id.clone(),
                        to_id: parent_id.clone(),
                        properties: None,
                        weight: None,
                    })
                    .await?;
                    result.edges_created += 1;
                }
            }
        }

        // Directory → files
        for (file_path, file_id) in &file_node_ids {
            let parent_path = Path::new(file_path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".to_string());

            if let Some(parent_id) = dir_node_ids.get(&parent_path) {
                self.create_relationship(RelationshipInput {
                    edge_type: RelationshipType::BelongsTo,
                    from_id: file_id.clone(),
                    to_id: parent_id.clone(),
                    properties: None,
                    weight: None,
                })
                .await?;
                result.edges_created += 1;
            }
        }

        // 5. Discover and parse build config files, create BuildSystem nodes
        let build_configs = analyzer_scanner::discover_build_files(project_root, false);
        let mut build_system_node_ids: Vec<String> = Vec::new();

        for (build_file, _parent_dir) in &build_configs {
            // Parse the build file using the existing build parsers
            if let Some(metadata) = build_parsers::parse_build_file(project_root, build_file, &[]) {
                // Build a description for the BuildSystem node
                let description = serde_json::json!({
                    "build_system": metadata.build_system,
                    "project_type": metadata.project_type,
                    "project_name": metadata.project_name,
                    "version": metadata.version,
                    "is_workspace": metadata.is_workspace,
                    "config_file": build_file,
                }).to_string();

                // Serialize scripts, dependencies, features, targets for storage
                let scripts_json = serde_json::to_value(&metadata.scripts).unwrap_or(serde_json::Value::Null);
                let features_json = serde_json::to_value(&metadata.features).unwrap_or(serde_json::Value::Null);
                let targets_json = serde_json::to_value(&metadata.targets).unwrap_or(serde_json::Value::Null);
                let workspace_members_json = serde_json::to_value(&metadata.workspace_members).unwrap_or(serde_json::Value::Null);
                let raw_json = metadata.raw.clone().unwrap_or(serde_json::Value::Null);

                // Extract dependencies from raw data if available
                let dependencies_json = raw_json.get("dependencies")
                    .cloned()
                    .or_else(|| {
                        // For Cargo, dependencies are in raw.dependencies
                        if metadata.build_system == "Cargo" {
                            raw_json.get("dependencies").cloned()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(serde_json::Value::Null);

                let build_system_name = format!("{}-{}", metadata.build_system, build_file.replace('/', "-"));

                let node = self
                    .create_node(NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("BuildSystem".to_string()),
                        name: build_system_name,
                        description: Some(description),
                        properties: Some({
                            let mut m = HashMap::new();
                            m.insert(
                                "build_type".to_string(),
                                serde_json::Value::String(metadata.build_system.clone()),
                            );
                            m.insert(
                                "project_type".to_string(),
                                serde_json::Value::String(metadata.project_type.clone()),
                            );
                            if let Some(ref name) = metadata.project_name {
                                m.insert(
                                    "project_name".to_string(),
                                    serde_json::Value::String(name.clone()),
                                );
                            }
                            if let Some(ref ver) = metadata.version {
                                m.insert(
                                    "version".to_string(),
                                    serde_json::Value::String(ver.clone()),
                                );
                            }
                            m.insert(
                                "is_workspace".to_string(),
                                serde_json::Value::Bool(metadata.is_workspace),
                            );
                            m.insert(
                                "config_file".to_string(),
                                serde_json::Value::String(build_file.clone()),
                            );
                            m.insert(
                                "scripts".to_string(),
                                scripts_json,
                            );
                            m.insert(
                                "features".to_string(),
                                features_json,
                            );
                            m.insert(
                                "targets".to_string(),
                                targets_json,
                            );
                            m.insert(
                                "workspace_members".to_string(),
                                workspace_members_json,
                            );
                            m.insert(
                                "dependencies".to_string(),
                                dependencies_json,
                            );
                            m
                        }),
                        embedding_id: None,
                    })
                    .await?;
                result.nodes_created += 1;
                build_system_node_ids.push(node.id.clone());

                // Link the BuildSystem node to the Project node
                self.create_relationship(RelationshipInput {
                    edge_type: RelationshipType::BelongsTo,
                    from_id: node.id.clone(),
                    to_id: project_node.id.clone(),
                    properties: None,
                    weight: None,
                })
                .await?;
                result.edges_created += 1;

                // Link the BuildSystem node to its build config file node (if it exists in the graph)
                if let Some(file_node_id) = file_node_ids.get(build_file) {
                    self.create_relationship(RelationshipInput {
                        edge_type: RelationshipType::DependsOn,
                        from_id: node.id.clone(),
                        to_id: file_node_id.clone(),
                        properties: None,
                        weight: None,
                    })
                    .await?;
                    result.edges_created += 1;
                }

                info!(
                    "ProjectSync: created BuildSystem node for {} ({}) at {}",
                    metadata.build_system, metadata.project_type, build_file
                );
            }
        }

        // Store build manifest hash
        let build_manifest_hash = {
            let mut hasher = Sha256::new();
            for (build_file, _) in &build_configs {
                if let Ok(content) = std::fs::read_to_string(project_root.join(build_file)) {
                    hasher.update(content.as_bytes());
                }
            }
            format!("{:x}", hasher.finalize())
        };

        // 6. Store the manifest hash and metadata
        self.set_config(
            CONFIG_FILE_MANIFEST_HASH,
            serde_json::Value::String(manifest_hash),
        )
        .await?;

        self.set_config(
            CONFIG_BUILD_MANIFEST_HASH,
            serde_json::Value::String(build_manifest_hash),
        )
        .await?;

        self.set_config(
            CONFIG_PROJECT_ROOT,
            serde_json::Value::String(project_root.to_string_lossy().to_string()),
        )
        .await?;

        self.set_config(
            CONFIG_LAST_SYNCED_AT,
            serde_json::Value::String(Utc::now().to_rfc3339()),
        )
        .await?;

        result.duration_ms = start.elapsed().as_millis() as u64;
        info!("ProjectSync: bootstrap complete — {:?}", result);
        Ok(result)
    }

    // ── Startup Sync ─────────────────────────────────────────────────────

    /// Phase 2: Startup sync — content-hash manifest diff.
    /// Only does work if the file manifest has changed.
    async fn startup_sync(&mut self, project_root: &Path) -> Result<SyncResult> {
        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        info!("ProjectSync: startup sync for {}", project_root.display());

        // 1. Scan the filesystem
        let manifest = Self::scan_manifest(project_root)?;
        let current_hash = hash_manifest(&manifest);

        // 2. Compare with stored hash
        let stored_hash = self
            .get_config(CONFIG_FILE_MANIFEST_HASH)
            .await?
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_default();

        if stored_hash == current_hash {
            info!("ProjectSync: manifest unchanged, skipping sync");
            result.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(result);
        }

        info!(
            "ProjectSync: manifest changed (old={} new={}), performing incremental sync",
            &stored_hash[..8.min(stored_hash.len())],
            &current_hash[..8.min(current_hash.len())]
        );

        // 3. Load the graph's current file nodes
        let graph_files = self
            .query_nodes(NodeFilter {
                node_type: None,
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await?;

        // Build a map of path → (node_id, size) from the graph
        let mut graph_file_map: HashMap<String, (String, u64)> = HashMap::new();
        for node in &graph_files {
            if let Some(path) = node.properties.get("path").and_then(|v| v.as_str()) {
                let size = node
                    .properties
                    .get("size")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                graph_file_map.insert(path.to_string(), (node.id.clone(), size));
            }
        }

        // 4. Build filesystem map
        let mut fs_file_map: HashMap<String, u64> = HashMap::new();
        for entry in &manifest {
            fs_file_map.insert(entry.path.clone(), entry.size);
        }

        // 5. Diff: files only in graph (deleted from filesystem)
        for (path, (node_id, _)) in &graph_file_map {
            if !fs_file_map.contains_key(path) {
                if let Err(e) = self.delete_node(node_id.clone()).await {
                    warn!("Failed to delete node for {}: {}", path, e);
                } else {
                    result.nodes_deleted += 1;
                }
            }
        }

        // 6. Diff: files only in filesystem (new files)
        for (path, size) in &fs_file_map {
            if !graph_file_map.contains_key(path) {
                let ext = Path::new(path)
                    .extension()
                    .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                    .unwrap_or_default();
                let language = Self::detect_language(&ext).to_string();
                let role = Self::classify_file_role(path);
                let lines = Self::estimate_lines(*size);
                let filename = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                match self
                    .create_node(NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("File".to_string()),
                        name: filename,
                        description: Some(Self::build_file_description(
                            path, &language, role, lines, *size,
                        )),
                        properties: Some({
                            let mut m = HashMap::new();
                            m.insert(
                                "path".to_string(),
                                serde_json::Value::String(path.clone()),
                            );
                            m.insert(
                                "extension".to_string(),
                                serde_json::Value::String(ext),
                            );
                            m.insert(
                                "language".to_string(),
                                serde_json::Value::String(language.clone()),
                            );
                            m.insert(
                                "role".to_string(),
                                serde_json::Value::String(role.to_string()),
                            );
                            m.insert(
                                "size".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(*size)),
                            );
                            m.insert(
                                "lines".to_string(),
                                serde_json::Value::Number(
                                    serde_json::Number::from(lines as u64),
                                ),
                            );
                            m
                        }),
                        embedding_id: None,
                    })
                    .await
                {
                    Ok(node) => {
                        result.nodes_created += 1;

                        // Generate embedding for new file
                        let desc = Self::build_file_description(
                            path, &language, role, lines, *size,
                        );
                        if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                            warn!(
                                "Failed to generate embedding for file {}: {}",
                                path, e
                            );
                        } else {
                            result.embeddings_generated += 1;
                        }

                        // Create BelongsTo edge to parent directory
                        let parent_path = Path::new(path)
                            .parent()
                            .map(|p| p.to_string_lossy().to_string())
                            .filter(|p| !p.is_empty())
                            .unwrap_or_else(|| ".".to_string());

                        // We need to find the parent directory node
                        // For now, skip edge creation during startup sync (will be handled by bootstrap on force-resync)
                        let _ = parent_path;
                    }
                    Err(e) => {
                        warn!("Failed to create node for {}: {}", path, e);
                    }
                }
            }
        }

        // 7. Update the stored hash
        self.set_config(
            CONFIG_FILE_MANIFEST_HASH,
            serde_json::Value::String(current_hash),
        )
        .await?;

        self.set_config(
            CONFIG_LAST_SYNCED_AT,
            serde_json::Value::String(Utc::now().to_rfc3339()),
        )
        .await?;

        result.duration_ms = start.elapsed().as_millis() as u64;
        info!("ProjectSync: startup sync complete — {:?}", result);
        Ok(result)
    }

    // ── Continuous Sync ──────────────────────────────────────────────────

    /// Phase 3: Handle a single file change event.
    /// This is called by the actor's message handler for each FileEvent.
    async fn handle_file_event(
        &mut self,
        change_type: ChangeType,
        path: &Path,
    ) -> Result<SyncResult> {
        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        debug!(
            "ProjectSync: file event {:?} for {}",
            change_type,
            path.display()
        );

        match change_type {
            ChangeType::Created => {
                // Create a new file node
                if !path.is_file() {
                    return Ok(result);
                }

                let relative = path
                    .to_string_lossy()
                    .to_string();
                let ext = path
                    .extension()
                    .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                    .unwrap_or_default();
                let language = Self::detect_language(&ext).to_string();
                let role = Self::classify_file_role(&relative);
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                let lines = Self::estimate_lines(size);
                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let node = self
                    .create_node(NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("File".to_string()),
                        name: filename,
                        description: Some(Self::build_file_description(
                            &relative, &language, role, lines, size,
                        )),
                        properties: Some({
                            let mut m = HashMap::new();
                            m.insert(
                                "path".to_string(),
                                serde_json::Value::String(relative.clone()),
                            );
                            m.insert(
                                "extension".to_string(),
                                serde_json::Value::String(ext),
                            );
                            m.insert(
                                "language".to_string(),
                                serde_json::Value::String(language.clone()),
                            );
                            m.insert(
                                "role".to_string(),
                                serde_json::Value::String(role.to_string()),
                            );
                            m.insert(
                                "size".to_string(),
                                serde_json::Value::Number(serde_json::Number::from(size)),
                            );
                            m.insert(
                                "lines".to_string(),
                                serde_json::Value::Number(
                                    serde_json::Number::from(lines as u64),
                                ),
                            );
                            m
                        }),
                        embedding_id: None,
                    })
                    .await?;
                result.nodes_created += 1;

                // Generate embedding
                let desc =
                    Self::build_file_description(&relative, &language, role, lines, size);
                if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                    warn!("Failed to generate embedding for {}: {}", relative, e);
                } else {
                    result.embeddings_generated += 1;
                }

                // Update manifest hash
                if let Some(project_root) = self
                    .get_config(CONFIG_PROJECT_ROOT)
                    .await?
                    .and_then(|v| v.as_str().map(|s| PathBuf::from(s)))
                {
                    if let Ok(manifest) = Self::scan_manifest(&project_root) {
                        let new_hash = hash_manifest(&manifest);
                        self.set_config(
                            CONFIG_FILE_MANIFEST_HASH,
                            serde_json::Value::String(new_hash),
                        )
                        .await?;
                    }
                }
            }
            ChangeType::Modified => {
                // Update existing file node
                let relative = path
                    .to_string_lossy()
                    .to_string();

                // Find the node by path property
                let nodes = self
                    .query_nodes(NodeFilter {
                        node_type: None,
                        subtype: Some("File".to_string()),
                        name: None,
                        status: None,
                        tags: None,
                        limit: None,
                        offset: None,
                    })
                    .await?;

                if let Some(node) = nodes.iter().find(|n| {
                    n.properties
                        .get("path")
                        .and_then(|v| v.as_str())
                        == Some(&relative)
                }) {
                    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    let lines = Self::estimate_lines(size);

                    self.update_node(
                        node.id.clone(),
                        NodeUpdate {
                            node_type: None,
                            subtype: None,
                            name: None,
                            description: None,
                            properties: Some({
                                let mut m = HashMap::new();
                                m.insert(
                                    "size".to_string(),
                                    serde_json::Value::Number(
                                        serde_json::Number::from(size),
                                    ),
                                );
                                m.insert(
                                    "lines".to_string(),
                                    serde_json::Value::Number(
                                        serde_json::Number::from(lines as u64),
                                    ),
                                );
                                m
                            }),
                            embedding_id: None,
                        },
                    )
                    .await?;
                    result.nodes_updated += 1;

                    // Re-generate embedding
                    let ext = path
                        .extension()
                        .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                        .unwrap_or_default();
                    let language = Self::detect_language(&ext).to_string();
                    let role = Self::classify_file_role(&relative);
                    let desc =
                        Self::build_file_description(&relative, &language, role, lines, size);
                    if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                        warn!("Failed to regenerate embedding for {}: {}", relative, e);
                    } else {
                        result.embeddings_generated += 1;
                    }
                }

                // Update manifest hash
                if let Some(project_root) = self
                    .get_config(CONFIG_PROJECT_ROOT)
                    .await?
                    .and_then(|v| v.as_str().map(|s| PathBuf::from(s)))
                {
                    if let Ok(manifest) = Self::scan_manifest(&project_root) {
                        let new_hash = hash_manifest(&manifest);
                        self.set_config(
                            CONFIG_FILE_MANIFEST_HASH,
                            serde_json::Value::String(new_hash),
                        )
                        .await?;
                    }
                }
            }
            ChangeType::Deleted => {
                // Find and delete the node
                let relative = path
                    .to_string_lossy()
                    .to_string();

                let nodes = self
                    .query_nodes(NodeFilter {
                        node_type: None,
                        subtype: Some("File".to_string()),
                        name: None,
                        status: None,
                        tags: None,
                        limit: None,
                        offset: None,
                    })
                    .await?;

                if let Some(node) = nodes.iter().find(|n| {
                    n.properties
                        .get("path")
                        .and_then(|v| v.as_str())
                        == Some(&relative)
                }) {
                    self.delete_node(node.id.clone()).await?;
                    result.nodes_deleted += 1;
                }

                // Update manifest hash
                if let Some(project_root) = self
                    .get_config(CONFIG_PROJECT_ROOT)
                    .await?
                    .and_then(|v| v.as_str().map(|s| PathBuf::from(s)))
                {
                    if let Ok(manifest) = Self::scan_manifest(&project_root) {
                        let new_hash = hash_manifest(&manifest);
                        self.set_config(
                            CONFIG_FILE_MANIFEST_HASH,
                            serde_json::Value::String(new_hash),
                        )
                        .await?;
                    }
                }
            }
        }

        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }
}

// ============================================================================
// Actor Trait Implementation
// ============================================================================

#[async_trait]
impl Actor for ProjectSyncActor {
    type Message = ProjectSyncMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            ProjectSyncMessage::Initialize {
                memory_graph_tx,
                embedder,
                reply_to,
            } => {
                info!("ProjectSyncActor: initializing");
                self.memory_graph_tx = Some(memory_graph_tx);
                self.embedder = Some(embedder);
                let _ = reply_to.send(Ok(()));
            }
            ProjectSyncMessage::Bootstrap {
                project_root,
                reply_to,
            } => {
                let result = self.bootstrap(&project_root).await;
                let _ = reply_to.send(result);
            }
            ProjectSyncMessage::StartupSync {
                project_root,
                reply_to,
            } => {
                let result = self.startup_sync(&project_root).await;
                let _ = reply_to.send(result);
            }
            ProjectSyncMessage::FileEvent {
                change_type,
                path,
                reply_to,
            } => {
                let result = self.handle_file_event(change_type, &path).await;
                let _ = reply_to.send(result);
            }
            ProjectSyncMessage::ForceResync {
                project_root,
                reply_to,
            } => {
                // Force resync = delete all project nodes and re-bootstrap
                info!(
                    "ProjectSync: force resync for {}",
                    project_root.display()
                );

                // Delete all File, Directory, and BuildSystem nodes
                for subtype in &["File", "Directory", "BuildSystem"] {
                    if let Ok(nodes) = self
                        .query_nodes(NodeFilter {
                            node_type: None,
                            subtype: Some(subtype.to_string()),
                            name: None,
                            status: None,
                            tags: None,
                            limit: None,
                            offset: None,
                        })
                        .await
                    {
                        for node in &nodes {
                            let _ = self.delete_node(node.id.clone()).await;
                        }
                    }
                }

                // Delete Project nodes
                if let Ok(projects) = self
                    .query_nodes(NodeFilter {
                        node_type: Some(NodeType::Project),
                        subtype: None,
                        name: None,
                        status: None,
                        tags: None,
                        limit: None,
                        offset: None,
                    })
                    .await
                {
                    for node in &projects {
                        let _ = self.delete_node(node.id.clone()).await;
                    }
                }

                // Re-bootstrap
                let result = self.bootstrap(&project_root).await;
                let _ = reply_to.send(result);
            }
        }
    }
}
