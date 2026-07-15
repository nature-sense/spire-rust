// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! ProjectQueryActor — high-level semantic project query tools for LLM understanding.
//!
//! This actor provides a set of tools that sit on top of the knowledge graph,
//! giving the LLM rich semantic understanding of the project structure, build
//! systems, languages, dependencies, and architecture.
//!
//! # Tools
//!
//! | Tool | Description |
//! |------|-------------|
//! | `project/getOverview` | High-level project summary |
//! | `project/getFileTree` | Directory/file tree with semantic annotations |
//! | `project/getFileDetails` | Detailed metadata about a specific file |
//! | `project/searchFiles` | Search files by name, language, role, or pattern |
//! | `project/getBuildConfig` | Parsed build configuration |
//! | `project/getDependencies` | Dependency graph (external + internal) |
//! | `project/getEntryPoints` | Main entry points of the project |
//! | `project/getArchitecture` | High-level architectural overview |
//! | `project/getEntities` | Functions, classes, types defined in the project |
//! | `project/getRelationships` | Relationships between project elements |
//! | `project/queryGraph` | Flexible graph query |
//! | `project/getChanges` | Recent file changes since last sync |

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, info, warn};

use crate::actors::Actor;
use crate::actors::memory_graph::MemoryGraphMessage;
use crate::actors::ToolInfo;
use crate::models::memory_graph::{
    GraphNode, NodeFilter, NodeType, RelationshipType, TraversalDirection, TraversalOptions,
};

// ============================================================================
// ProjectQueryMessage
// ============================================================================

/// Messages for the ProjectQuery actor.
pub enum ProjectQueryMessage {
    /// Initialize the actor with its dependencies.
    Initialize {
        memory_graph_tx: mpsc::Sender<MemoryGraphMessage>,
        project_root: PathBuf,
        reply_to: oneshot::Sender<anyhow::Result<()>>,
    },
    /// Handle a tool call.
    CallTool {
        tool: String,
        args: serde_json::Value,
        reply_to: oneshot::Sender<serde_json::Value>,
    },
    /// List the tools provided by this actor.
    ListTools {
        reply_to: oneshot::Sender<Vec<ToolInfo>>,
    },
}

// ============================================================================
// ProjectQueryActor
// ============================================================================

/// The ProjectQuery actor — semantic project query tools.
pub struct ProjectQueryActor {
    /// Sender to the MemoryGraph actor.
    memory_graph_tx: Option<mpsc::Sender<MemoryGraphMessage>>,
    /// Project root path.
    project_root: Option<PathBuf>,
}

impl ProjectQueryActor {
    pub fn new() -> Self {
        Self {
            memory_graph_tx: None,
            project_root: None,
        }
    }

    /// Return the tool definitions for this actor.
    pub fn tool_definitions() -> Vec<ToolInfo> {
        vec![
            ToolInfo {
                name: "project/getOverview".to_string(),
                description: "Get a high-level overview of the project — languages, build systems, directory structure, entry points, and total size.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolInfo {
                name: "project/getFileTree".to_string(),
                description: "Get the directory/file tree with semantic annotations. Optionally filter by role (source, test, config, docs, etc.) or path prefix.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "role": {"type": "string", "description": "Filter by directory or file role (e.g. 'source_code', 'tests', 'documentation', 'config')"},
                        "prefix": {"type": "string", "description": "Only include paths starting with this prefix"},
                        "maxDepth": {"type": "integer", "description": "Maximum directory depth (default: unlimited)"}
                    }
                }),
            },
            ToolInfo {
                name: "project/getFileDetails".to_string(),
                description: "Get detailed metadata about a specific file — language, role, line count, size, and any entities defined in it.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Relative file path within the project"}
                    },
                    "required": ["path"]
                }),
            },
            ToolInfo {
                name: "project/searchFiles".to_string(),
                description: "Search for files by name, language, role, or path pattern.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Search by filename (substring match)"},
                        "language": {"type": "string", "description": "Filter by programming language (e.g. 'Rust', 'TypeScript', 'Python')"},
                        "role": {"type": "string", "description": "Filter by file role (e.g. 'source', 'test', 'entry_point', 'build_config')"},
                        "pattern": {"type": "string", "description": "Glob-style path pattern (e.g. '**/*.rs')"},
                        "limit": {"type": "integer", "description": "Maximum results (default: 50)"}
                    }
                }),
            },
            ToolInfo {
                name: "project/getBuildConfig".to_string(),
                description: "Get parsed build configuration — build systems, dependencies, scripts, version info.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolInfo {
                name: "project/getDependencies".to_string(),
                description: "Get the dependency graph — both external dependencies and internal module relationships.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "description": "Filter by dependency type: 'external', 'internal', or 'all' (default: 'all')"}
                    }
                }),
            },
            ToolInfo {
                name: "project/getEntryPoints".to_string(),
                description: "Get the main entry points of the project — main functions, library entry points, CLI entry points.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolInfo {
                name: "project/getArchitecture".to_string(),
                description: "Get a high-level architectural overview — key modules, their responsibilities, and how they relate.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
            ToolInfo {
                name: "project/getEntities".to_string(),
                description: "Get entities (functions, classes, types, modules) defined in the project. Optionally filter by name, type, or file.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string", "description": "Search by entity name (substring match)"},
                        "entityType": {"type": "string", "description": "Filter by entity type (e.g. 'Function', 'Class', 'Interface', 'Module')"},
                        "file": {"type": "string", "description": "Only entities defined in this file path"},
                        "limit": {"type": "integer", "description": "Maximum results (default: 50)"}
                    }
                }),
            },
            ToolInfo {
                name: "project/getRelationships".to_string(),
                description: "Get relationships between project elements — call graphs, import graphs, dependency chains.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "nodeId": {"type": "string", "description": "Start from this node ID"},
                        "relationshipType": {"type": "string", "description": "Filter by relationship type (e.g. 'depends_on', 'called_by', 'belongs_to')"},
                        "direction": {"type": "string", "description": "Direction: 'in', 'out', or 'both' (default: 'out')"},
                        "depth": {"type": "integer", "description": "Max traversal depth (default: 1)"}
                    }
                }),
            },
            ToolInfo {
                name: "project/queryGraph".to_string(),
                description: "Flexible graph query — search nodes by type, subtype, name, or tags. Returns matching nodes with their relationships.".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "nodeType": {"type": "string", "description": "Filter by node type (e.g. 'Project', 'Entity', 'File', 'Directory', 'BuildSystem')"},
                        "subtype": {"type": "string", "description": "Filter by subtype (e.g. 'Function', 'Class', 'Module')"},
                        "name": {"type": "string", "description": "Search by name (substring match)"},
                        "tags": {"type": "array", "items": {"type": "string"}, "description": "Filter by tags"},
                        "limit": {"type": "integer", "description": "Maximum results (default: 50)"},
                        "includeRelationships": {"type": "boolean", "description": "Include relationships for each node (default: false)"}
                    }
                }),
            },
            ToolInfo {
                name: "project/getChanges".to_string(),
                description: "Get recent changes to the project — new, modified, or deleted files since the last sync.".to_string(),
                input_schema: serde_json::json!({"type": "object", "properties": {}}),
            },
        ]
    }

    // ── Graph Helpers ──────────────────────────────────────────────────────

    /// Send a message to the MemoryGraph actor and await the response.
    async fn send_to_graph<T, F>(&self, make_msg: F) -> anyhow::Result<T>
    where
        F: FnOnce(oneshot::Sender<anyhow::Result<T>>) -> MemoryGraphMessage,
    {
        let tx_ref = self.memory_graph_tx.as_ref().ok_or_else(|| {
            anyhow::anyhow!("MemoryGraph sender not initialized")
        })?;
        let (tx, rx) = oneshot::channel();
        tx_ref
            .send(make_msg(tx))
            .await
            .map_err(|e| anyhow::anyhow!("MemoryGraph channel closed: {}", e))?;
        rx.await
            .map_err(|e| anyhow::anyhow!("MemoryGraph response error: {}", e))?
    }

    /// Query nodes by filter.
    async fn query_nodes(&self, filter: NodeFilter) -> anyhow::Result<Vec<GraphNode>> {
        self.send_to_graph(|tx| MemoryGraphMessage::QueryNodes {
            filter,
            reply_to: tx,
        })
        .await
    }

    /// Get a single node by ID.
    #[allow(dead_code)]
    async fn get_node(&self, id: &str) -> anyhow::Result<Option<GraphNode>> {
        self.send_to_graph(|tx| MemoryGraphMessage::GetNode {
            id: id.to_string(),
            reply_to: tx,
        })
        .await
    }

    /// Get relationships for a node.
    async fn get_relationships(
        &self,
        node_id: &str,
    ) -> anyhow::Result<Vec<crate::models::memory_graph::GraphEdge>> {
        self.send_to_graph(|tx| MemoryGraphMessage::GetRelationships {
            node_id: node_id.to_string(),
            reply_to: tx,
        })
        .await
    }

    /// Traverse the graph from a start node.
    async fn traverse(
        &self,
        start_node_id: &str,
        options: TraversalOptions,
    ) -> anyhow::Result<crate::models::memory_graph::TraversalResult> {
        self.send_to_graph(|tx| MemoryGraphMessage::Traverse {
            start_node_id: start_node_id.to_string(),
            options,
            reply_to: tx,
        })
        .await
    }

    // ── Tool Implementations ──────────────────────────────────────────────

    /// `project/getOverview` — high-level project summary.
    async fn handle_get_overview(&self) -> serde_json::Value {
        // Find the Project node
        let project_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Project),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: Some(1),
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query project: {}", e)})
            }
        };

        let project = match project_nodes.first() {
            Some(p) => p,
            None => {
                return serde_json::json!(
                    {"error": "No project node found. Has the project been synced?"}
                )
            }
        };

        // Count files and directories
        let file_nodes = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        let dir_nodes = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("Directory".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        // Collect languages from file nodes
        let mut languages: HashMap<String, usize> = HashMap::new();
        for file in &file_nodes {
            if let Some(lang) = file.properties.get("language").and_then(|v| v.as_str()) {
                *languages.entry(lang.to_string()).or_insert(0) += 1;
            }
        }

        // Collect directory roles
        let mut dir_roles: HashMap<String, usize> = HashMap::new();
        for dir in &dir_nodes {
            if let Some(role) = dir.properties.get("role").and_then(|v| v.as_str()) {
                *dir_roles.entry(role.to_string()).or_insert(0) += 1;
            }
        }

        // Find build system nodes
        let build_systems = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("BuildSystem".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        // Find entry points
        let entry_points: Vec<String> = file_nodes
            .iter()
            .filter(|f| {
                f.properties
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|r| r == "entry_point")
                    .unwrap_or(false)
            })
            .map(|f| {
                f.properties
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&f.name)
                    .to_string()
            })
            .collect();

        // Compute totals
        let total_lines: usize = file_nodes
            .iter()
            .filter_map(|f| f.properties.get("lines").and_then(|v| v.as_u64()))
            .sum::<u64>() as usize;

        let mut lang_list: Vec<serde_json::Value> = languages
            .into_iter()
            .map(|(lang, count)| {
                serde_json::json!({
                    "language": lang,
                    "fileCount": count
                })
            })
            .collect();
        lang_list.sort_by(|a, b| {
            b["fileCount"]
                .as_u64()
                .cmp(&a["fileCount"].as_u64())
        });

        let mut dir_role_list: Vec<serde_json::Value> = dir_roles
            .into_iter()
            .map(|(role, count)| serde_json::json!({"role": role, "count": count}))
            .collect();
        dir_role_list.sort_by(|a, b| b["count"].as_u64().cmp(&a["count"].as_u64()));

        let build_system_list: Vec<serde_json::Value> = build_systems
            .iter()
            .map(|bs| {
                serde_json::json!({
                    "name": bs.name,
                    "type": bs.properties.get("build_type").and_then(|v| v.as_str()),
                    "projectName": bs.properties.get("project_name").and_then(|v| v.as_str()),
                    "version": bs.properties.get("version").and_then(|v| v.as_str()),
                })
            })
            .collect();

        serde_json::json!({
            "projectName": project.name,
            "projectRoot": project.properties.get("path"),
            "totalFiles": file_nodes.len(),
            "totalDirs": dir_nodes.len(),
            "totalLines": total_lines,
            "languages": lang_list,
            "directoryRoles": dir_role_list,
            "buildSystems": build_system_list,
            "entryPoints": entry_points,
        })
    }

    /// `project/getFileTree` — directory/file tree with semantic annotations.
    async fn handle_get_file_tree(&self, args: &serde_json::Value) -> serde_json::Value {
        let role_filter = args.get("role").and_then(|v| v.as_str());
        let prefix_filter = args.get("prefix").and_then(|v| v.as_str());
        let max_depth = args.get("maxDepth").and_then(|v| v.as_u64());

        // Get all directory nodes
        let dir_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("Directory".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query directories: {}", e)})
            }
        };

        // Get all file nodes
        let file_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query files: {}", e)})
            }
        };

        // Build a tree structure
        let mut tree: serde_json::Value = serde_json::json!({
            "name": "",
            "path": "",
            "type": "root",
            "children": []
        });

        // Index directories by path for role lookup
        let dir_roles: HashMap<&str, &str> = dir_nodes
            .iter()
            .filter_map(|d| {
                let path = d.properties.get("path").and_then(|v| v.as_str())?;
                let role = d.properties.get("role").and_then(|v| v.as_str())?;
                Some((path, role))
            })
            .collect();

        // Add files to tree
        for file in &file_nodes {
            let path = match file.properties.get("path").and_then(|v| v.as_str()) {
                Some(p) => p,
                None => continue,
            };

            // Apply filters
            if let Some(role) = role_filter {
                let file_role = file
                    .properties
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if file_role != role {
                    continue;
                }
            }
            if let Some(prefix) = prefix_filter {
                if !path.starts_with(prefix) {
                    continue;
                }
            }

            let parts: Vec<&str> = path.split('/').collect();
            let filename = parts.last().unwrap_or(&path);
            let dir_parts = &parts[..parts.len() - 1];

            let file_info = serde_json::json!({
                "name": filename,
                "path": path,
                "type": "file",
                "language": file.properties.get("language"),
                "role": file.properties.get("role"),
                "lines": file.properties.get("lines"),
            });

            add_to_tree(&mut tree, dir_parts, &file_info, 0, max_depth);
        }

        // Annotate directories with roles
        annotate_tree_roles(&mut tree, &dir_roles);

        tree
    }

    /// `project/getFileDetails` — detailed metadata about a specific file.
    async fn handle_get_file_details(&self, args: &serde_json::Value) -> serde_json::Value {
        let file_path = match args.get("path").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return serde_json::json!({"error": "Missing required parameter: 'path'"}),
        };

        // Find the file node by path
        let file_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query files: {}", e)})
            }
        };

        let file_node = match file_nodes.into_iter().find(|f| {
            f.properties
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| p == file_path)
                .unwrap_or(false)
        }) {
            Some(f) => f,
            None => {
                return serde_json::json!({"error": format!("File not found: {}", file_path)})
            }
        };

        // Get relationships for this file
        let relationships = self
            .get_relationships(&file_node.id)
            .await
            .unwrap_or_default();

        // Get entities defined in this file
        let entities = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|e| {
                e.properties
                    .get("file")
                    .and_then(|v| v.as_str())
                    .map(|f| f == file_path)
                    .unwrap_or(false)
            })
            .collect::<Vec<_>>();

        serde_json::json!({
            "id": file_node.id,
            "name": file_node.name,
            "path": file_node.properties.get("path"),
            "language": file_node.properties.get("language"),
            "role": file_node.properties.get("role"),
            "lines": file_node.properties.get("lines"),
            "size": file_node.properties.get("size"),
            "description": file_node.description,
            "relationships": relationships.iter().map(|r| {
                serde_json::json!({
                    "id": r.id,
                    "type": format!("{:?}", r.edge_type),
                    "fromId": r.from_id,
                    "toId": r.to_id,
                })
            }).collect::<Vec<_>>(),
            "entities": entities.iter().map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "subtype": e.subtype,
                    "description": e.description,
                })
            }).collect::<Vec<_>>(),
        })
    }

    /// `project/searchFiles` — search files by name, language, role, or pattern.
    async fn handle_search_files(&self, args: &serde_json::Value) -> serde_json::Value {
        let name_filter = args.get("name").and_then(|v| v.as_str());
        let language_filter = args.get("language").and_then(|v| v.as_str());
        let role_filter = args.get("role").and_then(|v| v.as_str());
        let pattern_filter = args.get("pattern").and_then(|v| v.as_str());
        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(50) as usize;

        let file_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query files: {}", e)})
            }
        };

        let results: Vec<serde_json::Value> = file_nodes
            .into_iter()
            .filter(|f| {
                let path = f
                    .properties
                    .get("path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let language = f
                    .properties
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let role = f
                    .properties
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                // Name filter (substring match on filename)
                if let Some(name) = name_filter {
                    let filename = std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("");
                    if !filename.to_lowercase().contains(&name.to_lowercase()) {
                        return false;
                    }
                }

                // Language filter
                if let Some(lang) = language_filter {
                    if !language.eq_ignore_ascii_case(lang) {
                        return false;
                    }
                }

                // Role filter
                if let Some(role_f) = role_filter {
                    if role != role_f {
                        return false;
                    }
                }

                // Pattern filter (simple glob-like match)
                if let Some(pattern) = pattern_filter {
                    if !glob_match::glob_match(pattern, path) {
                        return false;
                    }
                }

                true
            })
            .take(limit)
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "name": f.name,
                    "path": f.properties.get("path"),
                    "language": f.properties.get("language"),
                    "role": f.properties.get("role"),
                    "lines": f.properties.get("lines"),
                })
            })
            .collect();

        serde_json::json!({
            "total": results.len(),
            "results": results,
        })
    }

    /// `project/getBuildConfig` — parsed build configuration.
    async fn handle_get_build_config(&self) -> serde_json::Value {
        let build_systems = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("BuildSystem".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query build systems: {}", e)})
            }
        };

        let systems: Vec<serde_json::Value> = build_systems
            .iter()
            .map(|bs| {
                serde_json::json!({
                    "id": bs.id,
                    "name": bs.name,
                    "buildType": bs.properties.get("build_type"),
                    "projectName": bs.properties.get("project_name"),
                    "version": bs.properties.get("version"),
                    "scripts": bs.properties.get("scripts"),
                    "dependencies": bs.properties.get("dependencies"),
                })
            })
            .collect();

        serde_json::json!({
            "total": systems.len(),
            "buildSystems": systems,
        })
    }

    /// `project/getDependencies` — dependency graph.
    async fn handle_get_dependencies(&self, args: &serde_json::Value) -> serde_json::Value {
        let dep_type = args
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("all");

        // Find the project node
        let project_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Project),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: Some(1),
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query project: {}", e)})
            }
        };

        let project = match project_nodes.first() {
            Some(p) => p,
            None => return serde_json::json!({"error": "No project node found"}),
        };

        // Traverse depends_on relationships from the project
        let traversal = self
            .traverse(
                &project.id,
                TraversalOptions {
                    max_depth: 3,
                    relationship_types: Some(vec![RelationshipType::DependsOn]),
                    max_nodes: Some(200),
                    direction: Some(TraversalDirection::Out),
                },
            )
            .await
            .unwrap_or(crate::models::memory_graph::TraversalResult {
                nodes: vec![],
                edges: vec![],
                paths: vec![],
            });

        let mut external_deps: Vec<serde_json::Value> = Vec::new();
        let mut internal_deps: Vec<serde_json::Value> = Vec::new();

        for edge in &traversal.edges {
            let from_node = traversal.nodes.iter().find(|n| n.id == edge.from_id);
            let to_node = traversal.nodes.iter().find(|n| n.id == edge.to_id);

            let dep = serde_json::json!({
                "from": from_node.map(|n| n.name.as_str()).unwrap_or("unknown"),
                "fromId": edge.from_id,
                "to": to_node.map(|n| n.name.as_str()).unwrap_or("unknown"),
                "toId": edge.to_id,
                "type": format!("{:?}", edge.edge_type),
                "weight": edge.weight,
            });

            // Simple heuristic: if the target node has a version property, it's external
            let is_external = to_node
                .and_then(|n| n.properties.get("version"))
                .is_some();

            match dep_type {
                "external" => {
                    if is_external {
                        external_deps.push(dep);
                    }
                }
                "internal" => {
                    if !is_external {
                        internal_deps.push(dep);
                    }
                }
                _ => {
                    if is_external {
                        external_deps.push(dep.clone());
                    } else {
                        internal_deps.push(dep);
                    }
                }
            }
        }

        serde_json::json!({
            "external": external_deps,
            "internal": internal_deps,
            "totalExternal": external_deps.len(),
            "totalInternal": internal_deps.len(),
        })
    }

    /// `project/getEntryPoints` — main entry points.
    async fn handle_get_entry_points(&self) -> serde_json::Value {
        let file_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query files: {}", e)})
            }
        };

        let entry_points: Vec<serde_json::Value> = file_nodes
            .iter()
            .filter(|f| {
                f.properties
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|r| r == "entry_point")
                    .unwrap_or(false)
            })
            .map(|f| {
                serde_json::json!({
                    "id": f.id,
                    "name": f.name,
                    "path": f.properties.get("path"),
                    "language": f.properties.get("language"),
                    "lines": f.properties.get("lines"),
                })
            })
            .collect();

        serde_json::json!({
            "total": entry_points.len(),
            "entryPoints": entry_points,
        })
    }

    /// `project/getArchitecture` — high-level architectural overview.
    async fn handle_get_architecture(&self) -> serde_json::Value {
        // Get the project node
        let project_nodes = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Project),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: Some(1),
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => {
                return serde_json::json!({"error": format!("Failed to query project: {}", e)})
            }
        };

        let project = match project_nodes.first() {
            Some(p) => p,
            None => return serde_json::json!({"error": "No project node found"}),
        };

        // Get directory structure with roles
        let dir_nodes = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("Directory".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        // Group directories by role
        let mut dirs_by_role: HashMap<String, Vec<String>> = HashMap::new();
        for dir in &dir_nodes {
            let role = dir
                .properties
                .get("role")
                .and_then(|v| v.as_str())
                .unwrap_or("directory")
                .to_string();
            let path = dir
                .properties
                .get("path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            dirs_by_role.entry(role).or_default().push(path);
        }

        // Get build systems
        let build_systems = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("BuildSystem".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        // Get entry points
        let file_nodes = self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: Some("File".to_string()),
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
            .unwrap_or_default();

        let entry_points: Vec<&str> = file_nodes
            .iter()
            .filter(|f| {
                f.properties
                    .get("role")
                    .and_then(|v| v.as_str())
                    .map(|r| r == "entry_point")
                    .unwrap_or(false)
            })
            .filter_map(|f| f.properties.get("path").and_then(|v| v.as_str()))
            .collect();

        // Build the architecture summary
        let mut modules: Vec<serde_json::Value> = Vec::new();
        for (role, paths) in &dirs_by_role {
            let description = match role.as_str() {
                "source_code" => "Main source code directory",
                "tests" => "Test files and test infrastructure",
                "documentation" => "Project documentation",
                "build_scripts" => "Build scripts and tooling",
                "config" => "Configuration files",
                "examples" => "Example code and usage samples",
                "benchmarks" => "Performance benchmarks",
                "resources" => "Static resources and assets",
                "deployment" => "Deployment and CI/CD configuration",
                "extensions" => "Plugin and extension code",
                "database" => "Database migrations and schema",
                "localization" => "Internationalization and locale files",
                "build_output" => "Build output and compiled artifacts",
                "dependencies" => "Third-party dependencies",
                _ => "General directory",
            };

            modules.push(serde_json::json!({
                "role": role,
                "description": description,
                "directories": paths,
                "count": paths.len(),
            }));
        }

        serde_json::json!({
            "projectName": project.name,
            "projectRoot": project.properties.get("path"),
            "modules": modules,
            "buildSystems": build_systems.iter().map(|bs| {
                serde_json::json!({
                    "name": bs.name,
                    "type": bs.properties.get("build_type"),
                    "projectName": bs.properties.get("project_name"),
                })
            }).collect::<Vec<_>>(),
            "entryPoints": entry_points,
        })
    }

    /// `project/getEntities` — entities defined in the project.
    async fn handle_get_entities(&self, args: &serde_json::Value) -> serde_json::Value {
        let name_filter = args.get("name").and_then(|v| v.as_str());
        let entity_type_filter = args.get("entityType").and_then(|v| v.as_str());
        let file_filter = args.get("file").and_then(|v| v.as_str());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;

        // Query all entity nodes (Unknown type with various subtypes)
        let all_entities = match self
            .query_nodes(NodeFilter {
                node_type: Some(NodeType::Unknown),
                subtype: None,
                name: None,
                status: None,
                tags: None,
                limit: None,
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => return serde_json::json!({"error": format!("Failed to query entities: {}", e)}),
        };

        // Filter to only entity-like nodes (those with a subtype like Function, Class, etc.)
        let entity_subtypes = [
            "Function", "Class", "Interface", "Type", "Enum", "Struct",
            "Trait", "Module", "Method", "Variable", "Constant", "Macro",
        ];

        let results: Vec<serde_json::Value> = all_entities
            .into_iter()
            .filter(|e| {
                let subtype = e.subtype.as_deref().unwrap_or("");
                let is_entity = entity_subtypes.contains(&subtype);

                if !is_entity {
                    return false;
                }

                // Name filter
                if let Some(name) = name_filter {
                    if !e.name.to_lowercase().contains(&name.to_lowercase()) {
                        return false;
                    }
                }

                // Entity type filter
                if let Some(et) = entity_type_filter {
                    if !subtype.eq_ignore_ascii_case(et) {
                        return false;
                    }
                }

                // File filter
                if let Some(file) = file_filter {
                    let entity_file = e.properties.get("file").and_then(|v| v.as_str()).unwrap_or("");
                    if entity_file != file {
                        return false;
                    }
                }

                true
            })
            .take(limit)
            .map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "name": e.name,
                    "type": e.subtype,
                    "file": e.properties.get("file"),
                    "line": e.properties.get("line"),
                    "description": e.description,
                    "visibility": e.properties.get("visibility"),
                })
            })
            .collect();

        serde_json::json!({
            "total": results.len(),
            "entities": results,
        })
    }

    /// `project/getRelationships` — relationships between project elements.
    async fn handle_get_relationships(&self, args: &serde_json::Value) -> serde_json::Value {
        let node_id = args.get("nodeId").and_then(|v| v.as_str());
        let rel_type_filter = args.get("relationshipType").and_then(|v| v.as_str());
        let direction = args.get("direction").and_then(|v| v.as_str()).unwrap_or("out");
        let depth = args.get("depth").and_then(|v| v.as_u64()).unwrap_or(1) as u8;

        // If no node ID specified, use the project node
        let start_id = if let Some(id) = node_id {
            id.to_string()
        } else {
            let project_nodes = match self
                .query_nodes(NodeFilter {
                    node_type: Some(NodeType::Project),
                    subtype: None,
                    name: None,
                    status: None,
                    tags: None,
                    limit: Some(1),
                    offset: None,
                })
                .await
            {
                Ok(nodes) => nodes,
                Err(e) => return serde_json::json!({"error": format!("Failed to query project: {}", e)}),
            };

            match project_nodes.first() {
                Some(p) => p.id.clone(),
                None => return serde_json::json!({"error": "No project node found"}),
            }
        };

        // Parse relationship type filter
        let rel_types = rel_type_filter.map(|rt| {
            vec![match rt.to_lowercase().as_str() {
                "depends_on" => RelationshipType::DependsOn,
                "called_by" => RelationshipType::CalledBy,
                "belongs_to" => RelationshipType::BelongsTo,
                "imports" => RelationshipType::Unknown,
                _ => RelationshipType::Unknown,
            }]
        });

        // Parse direction
        let dir = match direction {
            "in" => Some(TraversalDirection::In),
            "both" => Some(TraversalDirection::Both),
            _ => Some(TraversalDirection::Out),
        };

        let traversal = match self
            .traverse(
                &start_id,
                TraversalOptions {
                    max_depth: depth,
                    relationship_types: rel_types,
                    max_nodes: Some(100),
                    direction: dir,
                },
            )
            .await
        {
            Ok(t) => t,
            Err(e) => return serde_json::json!({"error": format!("Traversal failed: {}", e)}),
        };

        serde_json::json!({
            "startNodeId": start_id,
            "nodes": traversal.nodes.iter().map(|n| {
                serde_json::json!({
                    "id": n.id,
                    "name": n.name,
                    "type": format!("{:?}", n.node_type),
                    "subtype": n.subtype,
                })
            }).collect::<Vec<_>>(),
            "relationships": traversal.edges.iter().map(|e| {
                serde_json::json!({
                    "id": e.id,
                    "type": format!("{:?}", e.edge_type),
                    "fromId": e.from_id,
                    "toId": e.to_id,
                    "weight": e.weight,
                })
            }).collect::<Vec<_>>(),
            "totalNodes": traversal.nodes.len(),
            "totalRelationships": traversal.edges.len(),
        })
    }

    /// `project/queryGraph` — flexible graph query.
    async fn handle_query_graph(&self, args: &serde_json::Value) -> serde_json::Value {
        let node_type_str = args.get("nodeType").and_then(|v| v.as_str());
        let subtype_filter = args.get("subtype").and_then(|v| v.as_str());
        let name_filter = args.get("name").and_then(|v| v.as_str());
        let tags_filter = args.get("tags").and_then(|v| v.as_array());
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        let include_rels = args.get("includeRelationships").and_then(|v| v.as_bool()).unwrap_or(false);

        // Parse node type
        let node_type = node_type_str.and_then(|nt| match nt.to_lowercase().as_str() {
            "project" => Some(NodeType::Project),
            "entity" => Some(NodeType::Entity),
            "decision" => Some(NodeType::Decision),
            "activecontext" | "active_context" => Some(NodeType::ActiveContext),
            "blocker" => Some(NodeType::Blocker),
            "milestone" => Some(NodeType::Milestone),
            "standard" => Some(NodeType::Standard),
            "conversation" => Some(NodeType::Conversation),
            "session" => Some(NodeType::Session),
            "mcpserver" | "mcp_server" => Some(NodeType::Unknown),
            _ => None,
        });

        let nodes = match self
            .query_nodes(NodeFilter {
                node_type,
                subtype: subtype_filter.map(|s| s.to_string()),
                name: name_filter.map(|n| n.to_string()),
                status: None,
                tags: tags_filter.map(|t| {
                    t.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                }),
                limit: Some(limit),
                offset: None,
            })
            .await
        {
            Ok(nodes) => nodes,
            Err(e) => return serde_json::json!({"error": format!("Query failed: {}", e)}),
        };

        // Optionally fetch relationships for each node
        let mut result_nodes: Vec<serde_json::Value> = Vec::new();
        for node in &nodes {
            let mut node_json = serde_json::json!({
                "id": node.id,
                "name": node.name,
                "type": format!("{:?}", node.node_type),
                "subtype": node.subtype,
                "description": node.description,
                "properties": node.properties,
            });

            if include_rels {
                let rels = self.get_relationships(&node.id).await.unwrap_or_default();
                node_json["relationships"] = serde_json::json!(rels.iter().map(|r| {
                    serde_json::json!({
                        "id": r.id,
                        "type": format!("{:?}", r.edge_type),
                        "fromId": r.from_id,
                        "toId": r.to_id,
                    })
                }).collect::<Vec<_>>());
            }

            result_nodes.push(node_json);
        }

        serde_json::json!({
            "total": result_nodes.len(),
            "nodes": result_nodes,
        })
    }

    /// `project/getChanges` — recent file changes.
    async fn handle_get_changes(&self) -> serde_json::Value {
        // Check if there's a sync manifest stored in config
        // TODO: retrieve from memory graph config when available
        let manifest: Option<serde_json::Value> = None;

        serde_json::json!({
            "hasManifest": manifest.is_some(),
            "manifest": manifest,
            "note": "Full change tracking requires the sync manifest to be stored. Currently shows basic sync status.",
        })
    }

    // ── Message Handler ───────────────────────────────────────────────────

    /// Handle an incoming message.
    pub async fn handle_message(&mut self, msg: ProjectQueryMessage) {
        match msg {
            ProjectQueryMessage::Initialize {
                memory_graph_tx,
                project_root,
                reply_to,
            } => {
                self.memory_graph_tx = Some(memory_graph_tx);
                self.project_root = Some(project_root);
                info!("ProjectQueryActor initialized");
                let _ = reply_to.send(Ok(()));
            }
            ProjectQueryMessage::CallTool {
                tool,
                args,
                reply_to,
            } => {
                let result = self.handle_tool_call(&tool, &args).await;
                let _ = reply_to.send(result);
            }
            ProjectQueryMessage::ListTools { reply_to } => {
                let _ = reply_to.send(Self::tool_definitions());
            }
        }
    }

    /// Route a tool call to the appropriate handler.
    async fn handle_tool_call(&self, tool: &str, args: &serde_json::Value) -> serde_json::Value {
        match tool {
            "project/getOverview" => self.handle_get_overview().await,
            "project/getFileTree" => self.handle_get_file_tree(args).await,
            "project/getFileDetails" => self.handle_get_file_details(args).await,
            "project/searchFiles" => self.handle_search_files(args).await,
            "project/getBuildConfig" => self.handle_get_build_config().await,
            "project/getDependencies" => self.handle_get_dependencies(args).await,
            "project/getEntryPoints" => self.handle_get_entry_points().await,
            "project/getArchitecture" => self.handle_get_architecture().await,
            "project/getEntities" => self.handle_get_entities(args).await,
            "project/getRelationships" => self.handle_get_relationships(args).await,
            "project/queryGraph" => self.handle_query_graph(args).await,
            "project/getChanges" => self.handle_get_changes().await,
            _ => serde_json::json!({"error": format!("Unknown tool: {}", tool)}),
        }
    }
}

// ============================================================================
// Actor trait implementation
// ============================================================================

#[async_trait]
impl Actor for ProjectQueryActor {
    type Message = ProjectQueryMessage;

    async fn handle(&mut self, msg: Self::Message) {
        self.handle_message(msg).await;
    }
}

// ============================================================================
// Free Functions
// ============================================================================

/// Recursively add a file to the tree structure.
fn add_to_tree(
    tree: &mut serde_json::Value,
    path_parts: &[&str],
    file_info: &serde_json::Value,
    depth: usize,
    max_depth: Option<u64>,
) {
    if let Some(max) = max_depth {
        if depth as u64 > max {
            return;
        }
    }

    if path_parts.is_empty() {
        if let Some(children) = tree.get_mut("children").and_then(|c| c.as_array_mut()) {
            children.push(file_info.clone());
        }
        return;
    }

    let dir_name = path_parts[0];
    let children = tree
        .get_mut("children")
        .and_then(|c| c.as_array_mut())
        .unwrap();

    let mut found = false;
    for child in children.iter_mut() {
        if child["name"] == dir_name && child["type"] == "directory" {
            add_to_tree(child, &path_parts[1..], file_info, depth + 1, max_depth);
            found = true;
            break;
        }
    }

    if !found {
        let mut dir_entry = serde_json::json!({
            "name": dir_name,
            "path": "",
            "type": "directory",
            "role": "",
            "children": []
        });
        add_to_tree(&mut dir_entry, &path_parts[1..], file_info, depth + 1, max_depth);
        children.push(dir_entry);
    }
}

/// Recursively annotate directories with their semantic roles.
fn annotate_tree_roles(tree: &mut serde_json::Value, dir_roles: &HashMap<&str, &str>) {
    // Extract parent path before mutable borrow
    let parent_path = tree["path"].as_str().unwrap_or("").to_string();

    if let Some(children) = tree.get_mut("children").and_then(|c| c.as_array_mut()) {
        for child in children.iter_mut() {
            if child["type"] == "directory" {
                // Build the path for this directory by combining parent path + name
                let dir_name = child["name"].as_str().unwrap_or("");
                let dir_path = if parent_path.is_empty() {
                    dir_name.to_string()
                } else {
                    format!("{}/{}", parent_path, dir_name)
                };
                child["path"] = serde_json::Value::String(dir_path.clone());

                // Look up the role
                if let Some(role) = dir_roles.get(dir_path.as_str()) {
                    child["role"] = serde_json::Value::String(role.to_string());
                }

                // Recurse into children
                annotate_tree_roles(child, dir_roles);
            }
        }
    }
}

