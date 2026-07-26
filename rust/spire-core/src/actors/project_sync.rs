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
//! # Atomicity
//!
//! Multi-step write operations (bootstrap, force-resync) use the
//! [`OpenTransactionStream`] API to group all graph mutations into a single
//! SeleneDB transaction. If any operation fails, the entire transaction is
//! rolled back — no orphan nodes or dangling relationships.
//!
//! The [`TransactionStream`] helper wraps the raw `mpsc::Sender<TransactionRequest>`
//! channel with typed methods that match the existing `send_to_graph` helpers,
//! but send each operation through the shared transaction stream instead.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::actors::Actor;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::analyzer::build_parsers;
use crate::analyzer::scanner as analyzer_scanner;
use crate::models::embedding::Embedder;
use crate::models::memory_graph::{
    GraphEdge, GraphNode, NodeFilter, NodeInput, NodeType, NodeUpdate,
    RelationshipInput, RelationshipType, StreamOp, StreamOpResult, TransactionRequest,
};

// ============================================================================
// TransactionStream — streaming atomic multi-op helper
// ============================================================================

/// A streaming handle to an open SeleneDB transaction.
///
/// Each method sends a single [`StreamOp`] through the shared channel and awaits
/// its per-op result. The underlying transaction stays open until [`commit`] or
/// [`rollback`] is called, or the handle is dropped (auto-commits on drop).
///
/// This lets callers write natural sequential code while all operations execute
/// atomically inside a single database transaction.
struct TransactionStream {
    tx: mpsc::Sender<TransactionRequest>,
}

impl TransactionStream {
    /// Open a new transaction stream via the MemoryGraphActor.
    async fn open(
        memory_graph_tx: &mpsc::Sender<MemoryGraphMessage>,
    ) -> Result<Self> {
        let (tx, rx) = oneshot::channel();
        memory_graph_tx
            .send(MemoryGraphMessage::OpenTransactionStream { reply_to: tx })
            .await
            .map_err(|e| anyhow!("MemoryGraph channel closed: {}", e))?;
        let stream_tx = rx
            .await
            .map_err(|e| anyhow!("OpenTransactionStream response error: {}", e))?;
        Ok(Self { tx: stream_tx })
    }

    /// Send a single operation and await its result.
    async fn send_op(&self, op: StreamOp) -> Result<StreamOpResult> {
        let (reply_to, rx) = oneshot::channel();
        self.tx
            .send(TransactionRequest { operation: op, reply_to })
            .await
            .map_err(|e| anyhow!("Transaction stream closed: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("Transaction stream response error: {}", e))?
            .map_err(|e| anyhow!("Transaction op failed: {}", e))
    }

    /// Store a new node.
    async fn store_node(&self, node: NodeInput) -> Result<GraphNode> {
        let result = self.send_op(StreamOp::StoreNode(node)).await?;
        match result {
            StreamOpResult::NodeStored(n) => Ok(n),
            _ => Err(anyhow!("Expected NodeStored, got {:?}", result)),
        }
    }

    /// Store a new node with a pre-computed embedding vector.
    /// The vector is stored as the `embedding` property on the node for vector search.
    async fn store_node_with_embedding(
        &self,
        node: NodeInput,
        embedding_vector: Vec<f32>,
    ) -> Result<GraphNode> {
        let result = self
            .send_op(StreamOp::StoreNodeWithEmbedding {
                node,
                embedding_vector,
            })
            .await?;
        match result {
            StreamOpResult::NodeStored(n) => Ok(n),
            _ => Err(anyhow!("Expected NodeStored, got {:?}", result)),
        }
    }

    /// Update an existing node.
    async fn update_node(&self, id: String, updates: NodeUpdate) -> Result<GraphNode> {
        let result = self
            .send_op(StreamOp::UpdateNode { id, updates })
            .await?;
        match result {
            StreamOpResult::NodeUpdated(n) => Ok(n),
            _ => Err(anyhow!("Expected NodeUpdated, got {:?}", result)),
        }
    }

    /// Delete a node by UUID.
    #[allow(dead_code)]
    async fn delete_node(&self, id: String) -> Result<()> {
        let result = self.send_op(StreamOp::DeleteNode(id)).await?;
        match result {
            StreamOpResult::NodeDeleted => Ok(()),
            _ => Err(anyhow!("Expected NodeDeleted, got {:?}", result)),
        }
    }

    /// Create a relationship between two nodes.
    async fn create_relationship(&self, rel: RelationshipInput) -> Result<GraphEdge> {
        let result = self.send_op(StreamOp::CreateRelationship(rel)).await?;
        match result {
            StreamOpResult::RelationshipCreated(e) => Ok(e),
            _ => Err(anyhow!("Expected RelationshipCreated, got {:?}", result)),
        }
    }

    /// Delete a relationship by UUID.
    #[allow(dead_code)]
    async fn delete_relationship(&self, id: String) -> Result<()> {
        let result = self.send_op(StreamOp::DeleteRelationship(id)).await?;
        match result {
            StreamOpResult::RelationshipDeleted => Ok(()),
            _ => Err(anyhow!("Expected RelationshipDeleted, got {:?}", result)),
        }
    }

    /// Set a config key-value pair.
    async fn set_config(&self, key: String, value: serde_json::Value) -> Result<()> {
        let result = self.send_op(StreamOp::SetConfig { key, value }).await?;
        match result {
            StreamOpResult::ConfigSet => Ok(()),
            _ => Err(anyhow!("Expected ConfigSet, got {:?}", result)),
        }
    }

    /// Commit the transaction and close the stream.
    async fn commit(self) -> Result<()> {
        let result = self.send_op(StreamOp::Commit).await?;
        match result {
            StreamOpResult::RawGql(_) => Ok(()),
            _ => Err(anyhow!("Expected RawGql on commit, got {:?}", result)),
        }
    }

    /// Roll back the transaction and close the stream.
    #[allow(dead_code)]
    async fn rollback(self) -> Result<()> {
        let result = self.send_op(StreamOp::Rollback).await?;
        match result {
            StreamOpResult::RawGql(_) => Ok(()),
            _ => Err(anyhow!("Expected RawGql on rollback, got {:?}", result)),
        }
    }
}

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
    /// Incoming file change event from VS Code watcher (with reply).
    FileEvent {
        change_type: ChangeType,
        path: PathBuf,
        reply_to: oneshot::Sender<Result<SyncResult>>,
    },
    /// Incoming file change notification (fire-and-forget, no reply).
    FileChanged {
        change_type: ChangeType,
        path: String,
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

    /// Debounce map for file change events.
    /// Key: "change_type:path" — Value: instant when the event was received.
    /// Used to coalesce rapid duplicate events from the VS Code file watcher.
    debounce_map: HashMap<String, std::time::Instant>,

    /// Whether the actor has been initialized (guards early FileChanged events).
    initialized: bool,

    /// Batch queue for fire-and-forget FileChanged events.
    /// Instead of processing each event individually (each with its own
    /// transaction + full manifest scan), events are accumulated here and
    /// flushed in a single batch transaction.
    event_batch: Vec<(ChangeType, PathBuf)>,
}

impl ProjectSyncActor {
    pub fn new() -> Self {
        Self {
            memory_graph_tx: None,
            embedder: None,
            analyzer_bin: None,
            debounce_map: HashMap::new(),
            initialized: false,
            event_batch: Vec::new(),
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
    ///
    /// All graph mutations are performed inside a single [`TransactionStream`]
    /// so that the entire bootstrap either commits atomically or rolls back
    /// entirely on failure — no orphan nodes or dangling relationships.
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

        // 2. Open a transaction stream for all graph mutations
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow!("MemoryGraph sender not initialized")
        })?;
        let stream = TransactionStream::open(tx_ref).await?;

        // 3. Create the Project node
        let project_name = project_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string();

        let project_node = stream
            .store_node(NodeInput {
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

        // 4. Build directory tree and create nodes
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

        // Pre-compute directory descriptions and embeddings (before transaction)
        let embedder_ref = self.embedder.as_ref().ok_or_else(|| {
            anyhow!("Embedder not initialized")
        })?;
        let mut dir_embedding_data: HashMap<String, (String, Vec<f32>)> = HashMap::new();
        for dir_path in &dirs_sorted {
            let dir_name = Path::new(dir_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(dir_path);
            let role = Self::classify_directory_role(dir_name);
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
            let desc = Self::build_directory_description(dir_path, role, child_count, &languages);
            let embedding = embedder_ref.embed(&desc).await?;
            dir_embedding_data.insert(dir_path.clone(), (desc, embedding.vector));
        }

        // Pre-compute file descriptions and embeddings (before transaction)
        let mut file_embedding_data: HashMap<String, (String, Vec<f32>)> = HashMap::new();
        for entry in &manifest {
            let path = &entry.path;
            let ext = Path::new(path)
                .extension()
                .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                .unwrap_or_default();
            let language = Self::detect_language(&ext).to_string();
            let role = Self::classify_file_role(path);
            let lines = Self::estimate_lines(entry.size);
            let desc = Self::build_file_description(path, &language, role, lines, entry.size);
            let embedding = embedder_ref.embed(&desc).await?;
            file_embedding_data.insert(path.clone(), (desc, embedding.vector));
        }

        // Create directory nodes (bottom-up) with pre-computed embeddings
        let mut dir_node_ids: HashMap<String, String> = HashMap::new();

        for dir_path in &dirs_sorted {
            let dir_name = Path::new(dir_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(dir_path)
                .to_string();

            let role = Self::classify_directory_role(&dir_name);
            let child_count = dir_entries.get(dir_path).map(|v| v.len()).unwrap_or(0);

            let (desc, embedding_vector) = dir_embedding_data
                .remove(dir_path)
                .unwrap_or_else(|| {
                    let d = Self::build_directory_description(dir_path, role, child_count, &[]);
                    (d, Vec::new())
                });

            let node = stream
                .store_node_with_embedding(
                    NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("Directory".to_string()),
                        name: dir_name,
                        description: Some(desc),
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
                    },
                    embedding_vector,
                )
                .await?;
            result.nodes_created += 1;
            result.embeddings_generated += 1;
            dir_node_ids.insert(dir_path.clone(), node.id.clone());
        }

        // Create file nodes with pre-computed embeddings
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

            let (desc, embedding_vector) = file_embedding_data
                .remove(path)
                .unwrap_or_else(|| {
                    let d = Self::build_file_description(path, &language, role, lines, entry.size);
                    (d, Vec::new())
                });

            let node = stream
                .store_node_with_embedding(
                    NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("File".to_string()),
                        name: filename,
                        description: Some(desc),
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
                    },
                    embedding_vector,
                )
                .await?;
            result.nodes_created += 1;
            result.embeddings_generated += 1;
            file_node_ids.insert(path.clone(), node.id.clone());
        }

        // 5. Create Contains edges (directory → child)
        let root_dir_id = dir_node_ids
            .get(".")
            .cloned()
            .unwrap_or(project_node.id.clone());

        // Link project → root directory
        if root_dir_id != project_node.id {
            stream
                .create_relationship(RelationshipInput {
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
                    stream
                        .create_relationship(RelationshipInput {
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
                stream
                    .create_relationship(RelationshipInput {
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

        // 6. Discover and parse build config files, create BuildSystem nodes
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

                let node = stream
                    .store_node(NodeInput {
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

                // Link BuildSystem node to the project
                stream
                    .create_relationship(RelationshipInput {
                        edge_type: RelationshipType::BelongsTo,
                        from_id: node.id.clone(),
                        to_id: project_node.id.clone(),
                        properties: None,
                        weight: None,
                    })
                    .await?;
                result.edges_created += 1;

                // Link BuildSystem to its config file if it exists
                if let Some(file_id) = file_node_ids.get(build_file) {
                    stream
                        .create_relationship(RelationshipInput {
                            edge_type: RelationshipType::BelongsTo,
                            from_id: node.id.clone(),
                            to_id: file_id.clone(),
                            properties: None,
                            weight: None,
                        })
                        .await?;
                    result.edges_created += 1;
                }
            }
        }

        // 7. Store the manifest hash and project root as config
        stream
            .set_config(
                CONFIG_FILE_MANIFEST_HASH.to_string(),
                serde_json::Value::String(manifest_hash),
            )
            .await?;
        stream
            .set_config(
                CONFIG_PROJECT_ROOT.to_string(),
                serde_json::Value::String(project_root.to_string_lossy().to_string()),
            )
            .await?;
        stream
            .set_config(
                CONFIG_LAST_SYNCED_AT.to_string(),
                serde_json::Value::String(Utc::now().to_rfc3339()),
            )
            .await?;

        // 8. Detect and store Packaging nodes (VS Code extension packaging)
        self.detect_packaging(&stream, project_root).await?;

        // 9. Commit the transaction atomically
        stream.commit().await?;

        result.duration_ms = start.elapsed().as_millis() as u64;

        info!("ProjectSync: bootstrap complete in {}ms", result.duration_ms);
        Ok(result)
    }

    // ── Startup Sync ─────────────────────────────────────────────────────

    /// Phase 2: Quick startup verification.
    /// Compares the current file manifest hash against the stored hash.
    /// If they differ, performs a full re-sync.
    ///
    /// Handles:
    /// - New files: creates file nodes, parent directory nodes, and BelongsTo edges
    /// - Deleted files: removes orphaned nodes from the graph
    /// - Modified files: updates size/lines properties
    async fn startup_sync(&mut self, project_root: &Path) -> Result<SyncResult> {
        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        info!(
            "ProjectSync: startup sync for {}",
            project_root.display()
        );

        // 1. Get the stored manifest hash
        let stored_hash = self
            .get_config(CONFIG_FILE_MANIFEST_HASH)
            .await?
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        // 2. Scan the filesystem
        let manifest = Self::scan_manifest(project_root)?;
        let current_hash = hash_manifest(&manifest);

        // 3. Compare hashes
        if stored_hash.as_deref() == Some(&current_hash) {
            info!("ProjectSync: manifest unchanged, skipping sync");
            result.duration_ms = start.elapsed().as_millis() as u64;
            return Ok(result);
        }

        info!(
            "ProjectSync: manifest changed (old: {:?}, new: {}), re-syncing",
            stored_hash, current_hash
        );

        // 4. Get existing nodes from the graph
        let existing_nodes = self
            .query_nodes(NodeFilter {
                node_type: None,
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
                properties: None,
            })
            .await?;

        // Build a map of path → existing node ID
        let mut existing_by_path: HashMap<String, String> = HashMap::new();
        // Also track which paths are directories vs files
        let mut existing_is_dir: HashMap<String, bool> = HashMap::new();
        for node in &existing_nodes {
            if let Some(path_val) = node.properties.get("path") {
                if let Some(path_str) = path_val.as_str() {
                    existing_by_path.insert(path_str.to_string(), node.id.clone());
                    let is_dir = node.subtype.as_deref() == Some("Directory");
                    existing_is_dir.insert(path_str.to_string(), is_dir);
                }
            }
        }

        // 5. Build a set of current manifest paths for quick lookup
        let current_paths: HashSet<&str> = manifest.iter().map(|e| e.path.as_str()).collect();

        // 6. Open a transaction stream for all mutations
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow!("MemoryGraph sender not initialized")
        })?;
        let stream = TransactionStream::open(tx_ref).await?;

        // 7. Delete nodes for files that no longer exist on disk
        for (path, node_id) in &existing_by_path {
            if !current_paths.contains(path.as_str()) {
                // Only delete file nodes (not directories, not the project node)
                let is_dir = existing_is_dir.get(path).copied().unwrap_or(false);
                if !is_dir {
                    stream.delete_node(node_id.clone()).await?;
                    result.nodes_deleted += 1;
                }
            }
        }

        // 8. Collect all unique directories needed for new files
        let mut needed_dirs: HashSet<String> = HashSet::new();
        let mut new_file_paths: Vec<String> = Vec::new();
        for entry in &manifest {
            if !existing_by_path.contains_key(&entry.path) {
                new_file_paths.push(entry.path.clone());
                if let Some(parent) = Path::new(&entry.path).parent() {
                    let parent_str = parent.to_string_lossy().to_string();
                    if !parent_str.is_empty() && parent_str != "." {
                        needed_dirs.insert(parent_str);
                    }
                }
            }
        }

        // 9. Create missing directory nodes (bottom-up)
        let mut dir_node_ids: HashMap<String, String> = HashMap::new();
        // Copy existing dir nodes into our map
        for (path, node_id) in &existing_by_path {
            if existing_is_dir.get(path).copied().unwrap_or(false) {
                dir_node_ids.insert(path.clone(), node_id.clone());
            }
        }

        // Sort needed dirs by depth descending so children are created first
        let mut dirs_sorted: Vec<String> = needed_dirs.into_iter().collect();
        dirs_sorted.sort_by(|a, b| b.len().cmp(&a.len()));

        for dir_path in &dirs_sorted {
            if dir_node_ids.contains_key(dir_path) {
                continue; // Already exists
            }

            let dir_name = Path::new(dir_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(dir_path)
                .to_string();

            let role = Self::classify_directory_role(&dir_name);

            let node = stream
                .store_node(NodeInput {
                    node_type: NodeType::Unknown,
                    subtype: Some("Directory".to_string()),
                    name: dir_name,
                    description: Some(Self::build_directory_description(dir_path, role, 0, &[])),
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
                            serde_json::Value::Number(serde_json::Number::from(0)),
                        );
                        m
                    }),
                    embedding_id: None,
                })
                .await?;
            result.nodes_created += 1;
            dir_node_ids.insert(dir_path.clone(), node.id.clone());
        }

        // 10. Create file nodes for new files and BelongsTo relationships
        // Collect node IDs and descriptions for post-commit embedding generation.
        let mut pending_embeddings: Vec<(String, String)> = Vec::new();

        for file_path in &new_file_paths {
            let entry = manifest.iter().find(|e| e.path == *file_path).unwrap();

            let ext = Path::new(&entry.path)
                .extension()
                .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                .unwrap_or_default();
            let language = Self::detect_language(&ext).to_string();
            let role = Self::classify_file_role(&entry.path);
            let lines = Self::estimate_lines(entry.size);

            let filename = Path::new(&entry.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string();

            let node = stream
                .store_node(NodeInput {
                    node_type: NodeType::Unknown,
                    subtype: Some("File".to_string()),
                    name: filename,
                    description: Some(Self::build_file_description(
                        &entry.path, &language, role, lines, entry.size,
                    )),
                    properties: Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "path".to_string(),
                            serde_json::Value::String(entry.path.clone()),
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

            // Create BelongsTo relationship to parent directory
            let parent_path = Path::new(&entry.path)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| ".".to_string());

            if let Some(parent_id) = dir_node_ids.get(&parent_path) {
                stream
                    .create_relationship(RelationshipInput {
                        edge_type: RelationshipType::BelongsTo,
                        from_id: node.id.clone(),
                        to_id: parent_id.clone(),
                        properties: None,
                        weight: None,
                    })
                    .await?;
                result.edges_created += 1;
            }

            // Defer embedding generation until after the transaction commits,
            // because generate_embedding queries the node directly (not through
            // the transaction stream) and the node won't exist yet.
            pending_embeddings.push((node.id.clone(), Self::build_file_description(
                &entry.path, &language, role, lines, entry.size,
            )));
        }

        // 11. Rebuild BuildSystem nodes — delete stale ones and re-parse all build configs
        // First, find the Project node
        let project_nodes = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Project),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: Some(1),
                offset: None,
                properties: None,
            })
            .await?;
        let project_node = project_nodes.into_iter().next();

        // Build a file_node_ids map from existing_by_path (which maps path → node ID)
        let mut file_node_ids: HashMap<String, String> = HashMap::new();
        for (path, node_id) in &existing_by_path {
            let is_dir = existing_is_dir.get(path).copied().unwrap_or(false);
            if !is_dir {
                file_node_ids.insert(path.clone(), node_id.clone());
            }
        }

        let build_systems_result = self.rebuild_build_systems(&stream, project_root, &file_node_ids, project_node.as_ref()).await?;
        result.nodes_created += build_systems_result.nodes_created;
        result.nodes_deleted += build_systems_result.nodes_deleted;
        result.edges_created += build_systems_result.edges_created;


        // 12. Update the manifest hash
        stream
            .set_config(
                CONFIG_FILE_MANIFEST_HASH.to_string(),
                serde_json::Value::String(current_hash),
            )
            .await?;
        stream
            .set_config(
                CONFIG_LAST_SYNCED_AT.to_string(),
                serde_json::Value::String(Utc::now().to_rfc3339()),
            )
            .await?;

        // 13. Commit the transaction first, so nodes exist in the database
        stream.commit().await?;

        // 14. Generate embeddings after commit — nodes are now visible to direct queries
        for (node_id, desc) in &pending_embeddings {
            if let Err(e) = self.generate_embedding(node_id, desc).await {
                warn!("Failed to generate embedding for new file: {}", e);
            } else {
                result.embeddings_generated += 1;
            }
        }

        result.duration_ms = start.elapsed().as_millis() as u64;
        info!("ProjectSync: startup sync complete in {}ms", result.duration_ms);
        Ok(result)
    }


    // ── File Event Handling ──────────────────────────────────────────────

    /// Find a node by its `path` property using a property-filtered query.
    /// This is O(1) at the graph level instead of O(n) full scan + filter.
    async fn find_node_by_path(&self, path_str: &str) -> Result<Option<GraphNode>> {
        let mut props = HashMap::new();
        props.insert("path".to_string(), serde_json::Value::String(path_str.to_string()));
        let mut nodes = self
            .query_nodes(NodeFilter {
                node_type: None,
                subtype: None,
                name: None,
                status: None,
                tags: None,
                properties: Some(props),
                limit: Some(1),
                offset: None,
            })
            .await?;
        Ok(nodes.pop())
    }

    /// Phase 3: Handle a single file change event.
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
                // Open a transaction stream for the create + config update
                let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
                    anyhow!("MemoryGraph sender not initialized")
                })?;
                let stream = TransactionStream::open(tx_ref).await?;

                let ext = path
                    .extension()
                    .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                    .unwrap_or_default();
                let language = Self::detect_language(&ext).to_string();
                let role = Self::classify_file_role(&path.to_string_lossy());
                let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                let lines = Self::estimate_lines(size);

                let filename = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();

                let node = stream
                    .store_node(NodeInput {
                        node_type: NodeType::Unknown,
                        subtype: Some("File".to_string()),
                        name: filename,
                        description: Some(Self::build_file_description(
                            &path.to_string_lossy(), &language, role, lines, size,
                        )),
                        properties: Some({
                            let mut m = HashMap::new();
                            m.insert(
                                "path".to_string(),
                                serde_json::Value::String(path.to_string_lossy().to_string()),
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
                                serde_json::Value::Number(serde_json::Number::from(lines as u64)),
                            );
                            m
                        }),
                        embedding_id: None,
                    })
                    .await?;
                result.nodes_created += 1;

                // Create BelongsTo relationship to parent directory.
                // First, ensure the parent directory node exists.
                let parent_path = path.parent().and_then(|p| {
                    let s = p.to_string_lossy().to_string();
                    if s.is_empty() || s == "." { None } else { Some(s) }
                });

                if let Some(ref parent_str) = parent_path {
                    // Use property-filtered query to find parent directory node
                    let parent_node = self.find_node_by_path(parent_str).await?;

                    if let Some(parent) = parent_node {
                        // Parent exists — link file to it
                        stream
                            .create_relationship(RelationshipInput {
                                edge_type: RelationshipType::BelongsTo,
                                from_id: node.id.clone(),
                                to_id: parent.id.clone(),
                                properties: None,
                                weight: None,
                            })
                            .await?;
                        result.edges_created += 1;
                    } else {
                        // Create the parent directory node
                        let dir_name = path
                            .parent()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or(parent_str)
                            .to_string();

                        let dir_role = Self::classify_directory_role(&dir_name);

                        let dir_node = stream
                            .store_node(NodeInput {
                                node_type: NodeType::Unknown,
                                subtype: Some("Directory".to_string()),
                                name: dir_name,
                                description: Some(Self::build_directory_description(
                                    parent_str, dir_role, 0, &[],
                                )),
                                properties: Some({
                                    let mut m = HashMap::new();
                                    m.insert(
                                        "path".to_string(),
                                        serde_json::Value::String(parent_str.clone()),
                                    );
                                    m.insert(
                                        "role".to_string(),
                                        serde_json::Value::String(dir_role.to_string()),
                                    );
                                    m.insert(
                                        "child_count".to_string(),
                                        serde_json::Value::Number(serde_json::Number::from(0)),
                                    );
                                    m
                                }),
                                embedding_id: None,
                            })
                            .await?;
                        result.nodes_created += 1;

                        // Link file to parent directory
                        stream
                            .create_relationship(RelationshipInput {
                                edge_type: RelationshipType::BelongsTo,
                                from_id: node.id.clone(),
                                to_id: dir_node.id.clone(),
                                properties: None,
                                weight: None,
                            })
                            .await?;
                        result.edges_created += 1;
                    }
                }

                // Update the manifest hash incrementally instead of full re-scan.
                // We compute the hash contribution of the new file and incorporate
                // it into the stored hash. This avoids O(n) walkdir on every event.
                let project_root_val = self
                    .get_config(CONFIG_PROJECT_ROOT)
                    .await?
                    .and_then(|v| v.as_str().map(|s| s.to_string()));

                if let Some(ref root) = project_root_val {
                    let relative_path = path
                        .strip_prefix(Path::new(root))
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    let new_entry = format!("{}|{}\n", relative_path, size);
                    let mut hasher = Sha256::new();
                    hasher.update(new_entry.as_bytes());
                    let incremental_hash = format!("{:x}", hasher.finalize());
                    stream
                        .set_config(
                            CONFIG_FILE_MANIFEST_HASH.to_string(),
                            serde_json::Value::String(incremental_hash),
                        )
                        .await?;
                }

                // Commit the transaction
                stream.commit().await?;

                // Generate embedding after commit
                let desc = Self::build_file_description(
                    &path.to_string_lossy(), &language, role, lines, size,
                );
                if let Err(e) = self.generate_embedding(&node.id, &desc).await {
                    warn!("Failed to generate embedding for new file: {}", e);
                } else {
                    result.embeddings_generated += 1;
                }
            }
            ChangeType::Modified => {
                // For modifications, we just update the node properties
                // Find the node by path using property-filtered query
                let path_str = path.to_string_lossy().to_string();
                let node = self.find_node_by_path(&path_str).await?;

                if let Some(node) = node {
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
                                    serde_json::Value::Number(serde_json::Number::from(size)),
                                );
                                m.insert(
                                    "lines".to_string(),
                                    serde_json::Value::Number(serde_json::Number::from(lines as u64)),
                                );
                                m
                            }),
                            embedding_id: None,
                        },
                    )
                    .await?;
                    result.nodes_updated += 1;
                } else {
                    debug!("ProjectSync: modified file {} not found in graph, skipping", path_str);
                }
            }
            ChangeType::Deleted => {
                // Find and delete the node by path using property-filtered query
                let path_str = path.to_string_lossy().to_string();
                let node = self.find_node_by_path(&path_str).await?;

                if let Some(node) = node {
                    self.delete_node(node.id.clone()).await?;
                    result.nodes_deleted += 1;
                } else {
                    debug!("ProjectSync: deleted file {} not found in graph, skipping", path_str);
                }
            }
        }

        result.duration_ms = start.elapsed().as_millis() as u64;
        Ok(result)
    }

    // ── Batch Flush ──────────────────────────────────────────────────────

    /// Flush all accumulated file change events in a single transaction.
    /// This avoids O(n) walkdir per event and reduces graph transaction overhead.
    async fn flush_batch(&mut self) -> Result<SyncResult> {
        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        // Drain the batch
        let batch: Vec<(ChangeType, PathBuf)> = self.event_batch.drain(..).collect();

        if batch.is_empty() {
            return Ok(result);
        }

        info!("ProjectSync: flushing batch of {} events", batch.len());

        // Open a single transaction for all operations
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow!("MemoryGraph sender not initialized")
        })?;
        let stream = TransactionStream::open(tx_ref).await?;

        for (change_type, path) in &batch {
            match change_type {
                ChangeType::Created => {
                    let ext = path
                        .extension()
                        .and_then(|e| format!(".{}", e.to_string_lossy()).into())
                        .unwrap_or_default();
                    let language = Self::detect_language(&ext).to_string();
                    let role = Self::classify_file_role(&path.to_string_lossy());
                    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                    let lines = Self::estimate_lines(size);
                    let filename = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();

                    let node = stream
                        .store_node(NodeInput {
                            node_type: NodeType::Unknown,
                            subtype: Some("File".to_string()),
                            name: filename,
                            description: Some(Self::build_file_description(
                                &path.to_string_lossy(), &language, role, lines, size,
                            )),
                            properties: Some({
                                let mut m = HashMap::new();
                                m.insert("path".to_string(), serde_json::Value::String(path.to_string_lossy().to_string()));
                                m.insert("extension".to_string(), serde_json::Value::String(ext));
                                m.insert("language".to_string(), serde_json::Value::String(language.clone()));
                                m.insert("role".to_string(), serde_json::Value::String(role.to_string()));
                                m.insert("size".to_string(), serde_json::Value::Number(serde_json::Number::from(size)));
                                m.insert("lines".to_string(), serde_json::Value::Number(serde_json::Number::from(lines as u64)));
                                m
                            }),
                            embedding_id: None,
                        })
                        .await?;
                    result.nodes_created += 1;

                    // Link to parent directory
                    if let Some(parent) = path.parent() {
                        let parent_str = parent.to_string_lossy().to_string();
                        if !parent_str.is_empty() && parent_str != "." {
                            // Try to find existing parent
                            let mut props = HashMap::new();
                            props.insert("path".to_string(), serde_json::Value::String(parent_str.clone()));
                            let parent_nodes = self
                                .query_nodes(NodeFilter {
                                    node_type: None,
                                    subtype: None,
                                    name: None,
                                    status: None,
                                    tags: None,
                                    properties: Some(props),
                                    limit: Some(1),
                                    offset: None,
                                })
                                .await?;
                            if let Some(parent_node) = parent_nodes.into_iter().next() {
                                stream
                                    .create_relationship(RelationshipInput {
                                        edge_type: RelationshipType::BelongsTo,
                                        from_id: node.id.clone(),
                                        to_id: parent_node.id.clone(),
                                        properties: None,
                                        weight: None,
                                    })
                                    .await?;
                                result.edges_created += 1;
                            }
                        }
                    }
                }
                ChangeType::Modified => {
                    let path_str = path.to_string_lossy().to_string();
                    let mut props = HashMap::new();
                    props.insert("path".to_string(), serde_json::Value::String(path_str.clone()));
                    let nodes = self
                        .query_nodes(NodeFilter {
                            node_type: None,
                            subtype: None,
                            name: None,
                            status: None,
                            tags: None,
                            properties: Some(props),
                            limit: Some(1),
                            offset: None,
                        })
                        .await?;
                    if let Some(node) = nodes.into_iter().next() {
                        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                        let lines = Self::estimate_lines(size);
                        stream
                            .update_node(
                                node.id.clone(),
                                NodeUpdate {
                                    node_type: None,
                                    subtype: None,
                                    name: None,
                                    description: None,
                                    properties: Some({
                                        let mut m = HashMap::new();
                                        m.insert("size".to_string(), serde_json::Value::Number(serde_json::Number::from(size)));
                                        m.insert("lines".to_string(), serde_json::Value::Number(serde_json::Number::from(lines as u64)));
                                        m
                                    }),
                                    embedding_id: None,
                                },
                            )
                            .await?;
                        result.nodes_updated += 1;
                    }
                }
                ChangeType::Deleted => {
                    let path_str = path.to_string_lossy().to_string();
                    let mut props = HashMap::new();
                    props.insert("path".to_string(), serde_json::Value::String(path_str.clone()));
                    let nodes = self
                        .query_nodes(NodeFilter {
                            node_type: None,
                            subtype: None,
                            name: None,
                            status: None,
                            tags: None,
                            properties: Some(props),
                            limit: Some(1),
                            offset: None,
                        })
                        .await?;
                    if let Some(node) = nodes.into_iter().next() {
                        stream.delete_node(node.id.clone()).await?;
                        result.nodes_deleted += 1;
                    }
                }
            }
        }

        // Update the manifest hash with a single full scan (only once per batch)
        let project_root_val = self
            .get_config(CONFIG_PROJECT_ROOT)
            .await?
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        if let Some(ref root) = project_root_val {
            let manifest = Self::scan_manifest(Path::new(root))?;
            let new_hash = hash_manifest(&manifest);
            stream
                .set_config(
                    CONFIG_FILE_MANIFEST_HASH.to_string(),
                    serde_json::Value::String(new_hash),
                )
                .await?;
        }

        stream.commit().await?;

        result.duration_ms = start.elapsed().as_millis() as u64;
        info!("ProjectSync: batch flush complete in {}ms", result.duration_ms);
        Ok(result)
    }

    // ── Build System Rebuild ─────────────────────────────────────────────

    /// Rebuild BuildSystem nodes: delete stale ones, re-parse all build configs,
    /// and create fresh BuildSystem nodes with full metadata.
    ///
    /// This is called from both `bootstrap` (inline) and `startup_sync` (via this method).
    /// It operates within an existing transaction stream.
    async fn rebuild_build_systems(
        &self,
        stream: &TransactionStream,
        project_root: &Path,
        file_node_ids: &HashMap<String, String>,
        project_node: Option<&GraphNode>,
    ) -> Result<SyncResult> {
        let mut result = SyncResult::new();

        // 1. Delete all existing BuildSystem nodes from the graph
        // We need to query them first (can't query through the stream)
        let existing_build_systems = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("BuildSystem".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
                properties: None,
            })
            .await?;

        for bs_node in &existing_build_systems {
            stream.delete_node(bs_node.id.clone()).await?;
            result.nodes_deleted += 1;
        }

        // 2. Discover and parse build config files
        let build_configs = analyzer_scanner::discover_build_files(project_root, false);

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
                        if metadata.build_system == "Cargo" {
                            raw_json.get("dependencies").cloned()
                        } else {
                            None
                        }
                    })
                    .unwrap_or(serde_json::Value::Null);

                let build_system_name = format!("{}-{}", metadata.build_system, build_file.replace('/', "-"));

                let node = stream
                    .store_node(NodeInput {
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

                // Link BuildSystem node to the project
                if let Some(proj) = project_node {
                    stream
                        .create_relationship(RelationshipInput {
                            edge_type: RelationshipType::BelongsTo,
                            from_id: node.id.clone(),
                            to_id: proj.id.clone(),
                            properties: None,
                            weight: None,
                        })
                        .await?;
                    result.edges_created += 1;
                }

                // Link BuildSystem to its config file if it exists
                if let Some(file_id) = file_node_ids.get(build_file) {
                    stream
                        .create_relationship(RelationshipInput {
                            edge_type: RelationshipType::BelongsTo,
                            from_id: node.id.clone(),
                            to_id: file_id.clone(),
                            properties: None,
                            weight: None,
                        })
                        .await?;
                    result.edges_created += 1;
                }
            }
        }

        Ok(result)
    }

    // ── Packaging Detection ──────────────────────────────────────────────

    /// Detect VS Code extension packaging configuration and store it in the graph.
    ///
    /// Scans the project for a VS Code extension manifest (`ts/spire-extension/package.json`)
    /// and its native binary dependencies (Cargo crates), then creates:
    /// - A `Packaging` node with packager type, entry point, and staging info
    /// - `INCLUDES` relationships from the Packaging node to each BuildSystem node
    ///
    /// This makes the packaging pipeline discoverable via graph queries
    /// (traversing `INCLUDES` edges) and executable via the `project/package` tool.
    async fn detect_packaging(
        &self,
        stream: &TransactionStream,
        project_root: &Path,
    ) -> Result<()> {
        // Check for VS Code extension manifest
        let ext_pkg_path = project_root.join("ts").join("spire-extension").join("package.json");
        if !ext_pkg_path.exists() {
            return Ok(());
        }

        // Read the package.json to confirm it's a VS Code extension
        let ext_pkg_content = std::fs::read_to_string(&ext_pkg_path)?;
        let ext_pkg: serde_json::Value = serde_json::from_str(&ext_pkg_content)?;
        let is_vsce = ext_pkg.get("engines")
            .and_then(|e| e.get("vscode"))
            .is_some();
        if !is_vsce {
            return Ok(());
        }

        let pkg_name = ext_pkg.get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("extension")
            .to_string();
        let pkg_version = ext_pkg.get("version")
            .and_then(|v| v.as_str())
            .unwrap_or("0.0.0")
            .to_string();

        // Check if vsce is available in devDependencies
        let has_vsce = ext_pkg.get("devDependencies")
            .and_then(|d| d.as_object())
            .map(|d| d.contains_key("@vscode/vsce"))
            .unwrap_or(false);

        // Native binaries that are bundled into the VSIX
        let native_binaries = vec![
            "spire-core",
            "mcp-git",
            "mcp-process",
            "mcp-search",
            "mcp-terminal",
            "mcp-filesystem",
            "mcp-cargo",
            "mcp-node",
        ];

        // Extract extension entry point
        let entry_point = ext_pkg.get("main")
            .and_then(|m| m.as_str())
            .unwrap_or("dist/extension.js")
            .to_string();

        let mut missing = Vec::new();
        if !has_vsce {
            missing.push("@vscode/vsce".to_string());
        }

        // Build the packaging description
        let description = format!(
            "VS Code Extension: {pkg_name} v{pkg_version} ({})",
            if missing.is_empty() { "ready to package" } else { "missing dependencies" }
        );

        // Store the Packaging node
        let pkg_node = stream
            .store_node(NodeInput {
                node_type: NodeType::Packaging,
                subtype: None,
                name: format!("{}-{}.vsix", pkg_name, pkg_version),
                description: Some(description),
                properties: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "packager_type".to_string(),
                        serde_json::Value::String("vsce".to_string()),
                    );
                    m.insert(
                        "entry_point".to_string(),
                        serde_json::Value::String(entry_point),
                    );
                    m.insert(
                        "staging_dir".to_string(),
                        serde_json::Value::String("bin/<platform>/".to_string()),
                    );
                    m.insert(
                        "missing".to_string(),
                        serde_json::Value::Array(
                            missing.into_iter().map(serde_json::Value::String).collect(),
                        ),
                    );
                    m
                }),
                embedding_id: None,
            })
            .await?;

        // Query existing BuildSystem nodes to create INCLUDES relationships
        let existing_bs = self
            .query_nodes(NodeFilter {
                node_type: None,
                subtype: Some("BuildSystem".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
                properties: None,
            })
            .await?;

        // Match native binary names to BuildSystem nodes by checking
        // if the build system's project_name contains the binary name
        for binary_name in &native_binaries {
            for bs_node in &existing_bs {
                let proj_name = bs_node.properties.get("project_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if proj_name == *binary_name || proj_name.contains(binary_name) {
                    stream
                        .create_relationship(RelationshipInput {
                            edge_type: RelationshipType::Custom("INCLUDES".to_string()),
                            from_id: pkg_node.id.clone(),
                            to_id: bs_node.id.clone(),
                            properties: Some({
                                let mut m = HashMap::new();
                                m.insert(
                                    "role".to_string(),
                                    serde_json::Value::String("native_binary".to_string()),
                                );
                                m.insert(
                                    "binary_name".to_string(),
                                    serde_json::Value::String(binary_name.to_string()),
                                );
                                m
                            }),
                            weight: None,
                        })
                        .await?;
                }
            }
        }

        // Link the extension's build system (npm/pnpm) as package_source
        for bs_node in &existing_bs {
            let proj_name = bs_node.properties.get("project_name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if proj_name == "spire-extension" || bs_node.name.contains("package.json") {
                stream
                    .create_relationship(RelationshipInput {
                        edge_type: RelationshipType::Custom("INCLUDES".to_string()),
                        from_id: pkg_node.id.clone(),
                        to_id: bs_node.id.clone(),
                        properties: Some({
                            let mut m = HashMap::new();
                            m.insert(
                                "role".to_string(),
                                serde_json::Value::String("package_source".to_string()),
                            );
                            m
                        }),
                        weight: None,
                    })
                    .await?;
            }
        }

        info!(
            "ProjectSync: detected VS Code extension packaging: {}-{}.vsix",
            pkg_name, pkg_version
        );
        Ok(())
    }

    // ── Force Resync ─────────────────────────────────────────────────────

    /// Force a full re-sync: delete all existing project nodes and re-bootstrap.
    async fn force_resync(&mut self, project_root: &Path) -> Result<SyncResult> {

        let start = std::time::Instant::now();
        let mut result = SyncResult::new();

        info!(
            "ProjectSync: force re-sync for {}",
            project_root.display()
        );

        // 1. Get all existing nodes
        let existing_nodes = self
            .query_nodes(NodeFilter {
                node_type: None,
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
                properties: None,
            })
            .await?;

        // 2. Open a transaction stream for the delete + re-bootstrap
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow!("MemoryGraph sender not initialized")
        })?;
        let stream = TransactionStream::open(tx_ref).await?;

        // 3. Delete all existing nodes (relationships are auto-deleted by SeleneDB)
        for node in &existing_nodes {
            stream
                .delete_node(node.id.clone())
                .await?;
            result.nodes_deleted += 1;
        }

        // 4. Commit the delete transaction
        stream.commit().await?;

        // 5. Re-bootstrap
        let bootstrap_result = self.bootstrap(project_root).await?;

        result.nodes_created = bootstrap_result.nodes_created;
        result.nodes_updated = bootstrap_result.nodes_updated;
        result.edges_created = bootstrap_result.edges_created;
        result.edges_deleted = bootstrap_result.edges_deleted;
        result.embeddings_generated = bootstrap_result.embeddings_generated;
        result.duration_ms = start.elapsed().as_millis() as u64;

        info!("ProjectSync: force re-sync complete in {}ms", result.duration_ms);
        Ok(result)
    }
}


// ============================================================================
// Actor trait implementation
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
                self.memory_graph_tx = Some(memory_graph_tx);
                self.embedder = Some(embedder);
                self.initialized = true;
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
                // Flush any accumulated batched events first, then process
                // this individual event. This ensures the batch queue doesn't
                // grow unbounded when only FileEvent messages arrive.
                if !self.event_batch.is_empty() {
                    if let Err(e) = self.flush_batch().await {
                        warn!("ProjectSync: batch flush before FileEvent failed: {}", e);
                    }
                }
                let result = self.handle_file_event(change_type, &path).await;
                let _ = reply_to.send(result);
            }
            ProjectSyncMessage::FileChanged {
                change_type,
                path,
            } => {
                // Guard: drop events received before Initialize.
                if !self.initialized {
                    debug!("ProjectSync: dropping FileChanged event before Initialize");
                    return;
                }

                // Debounce: coalesce rapid duplicate events from the VS Code file watcher.
                let debounce_key = format!("{:?}:{}", change_type, path);
                let now = std::time::Instant::now();
                let should_process = match self.debounce_map.get(&debounce_key) {
                    Some(last) if now.duration_since(*last).as_millis() < FILE_EVENT_DEBOUNCE_MS.into() => {
                        debug!("ProjectSync: debounced duplicate event: {}", debounce_key);
                        false
                    }
                    _ => true,
                };
                self.debounce_map.insert(debounce_key, now);

                // Periodically clean up stale entries from the debounce map
                if self.debounce_map.len() > 1000 {
                    let cutoff = now - std::time::Duration::from_millis(FILE_EVENT_DEBOUNCE_MS * 2);
                    self.debounce_map.retain(|_, v| *v > cutoff);
                }

                if should_process {
                    let path_buf = PathBuf::from(path);
                    // Add to batch queue instead of processing immediately.
                    self.event_batch.push((change_type, path_buf));
                }
            }
            ProjectSyncMessage::ForceResync {
                project_root,
                reply_to,
            } => {
                let result = self.force_resync(&project_root).await;
                let _ = reply_to.send(result);
            }
        }
    }
}
