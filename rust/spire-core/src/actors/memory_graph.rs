// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! MemoryGraphActor — backed by SeleneDB's `GraphDb` with GQL persistence.
//!
//! This actor is the sole data store for the system, owning graph nodes, edges,
//! and vector embeddings. All storage is delegated to `GraphDb` (SeleneDB),
//! which provides lock-free reads, serialized writes, and optional WAL persistence.
//!
//! All data access is through GQL statements via `execute_gql_write` and
//! `execute_gql_query`. No low-level SharedGraph API is used.
//!
//! # ID Mapping
//!
//! The external API uses UUID-based `String` IDs (for compatibility with the
//! TypeScript extension), while SeleneDB uses compact `u64` IDs (`NodeId`/`EdgeId`).
//! This actor maintains a bidirectional mapping between the two, stored as
//! properties on the nodes/edges themselves.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use selene_db_core::value::Value;
use selene_db_core::vector::VectorMetric;
use selene_db_gql::runtime::StatementOutput;

use crate::actors::Actor;
use crate::graph::GraphDb;
use crate::models::embedding::Embedder;

use crate::models::memory_graph::{
    ContextSearchResult, GraphEdge, GraphNode, McpConfigFile,
    McpServerConfigEntry, MemoryEntry, MemoryMetadata, NodeFilter, NodeInput, NodeType, NodeUpdate,
    ProjectSnapshot, ProjectStats, RelationshipInput, RelationshipType, RetrievalSource,
    ScoredNode, SearchOptions, StreamOp, StreamOpResult, TraversalDirection, TraversalOptions, TraversalPath,
    TraversalResult, TransactionRequest,
};


// ============================================================================
// GQL Schema Constants
// ============================================================================

/// Label used for all Spire graph nodes in SeleneDB.
const LABEL_SPIRE_NODE: &str = "SpireNode";
/// Label used for config key-value storage nodes.
const LABEL_CONFIG: &str = "SpireConfig";
/// Property key for the UUID string.
const PROP_UUID: &str = "uuid";
/// Property key for the node type.
const PROP_NODE_TYPE: &str = "node_type";
/// Property key for the node subtype.
const PROP_SUBTYPE: &str = "subtype";
/// Property key for the node name.
const PROP_NAME: &str = "name";
/// Property key for the node description.
const PROP_DESCRIPTION: &str = "description";
/// Property key for the embedding ID.
const PROP_EMBEDDING_ID: &str = "embedding_id";
/// Property key for the created_at timestamp.
const PROP_CREATED_AT: &str = "created_at";
/// Property key for the updated_at timestamp.
const PROP_UPDATED_AT: &str = "updated_at";
/// Property key for the version number.
const PROP_VERSION: &str = "version";
/// Property key for the edge type.
const PROP_EDGE_TYPE: &str = "edge_type";
/// Property key for the edge weight.
const PROP_WEIGHT: &str = "weight";
/// Property key for the config value.
const PROP_CONFIG_VALUE: &str = "config_value";

/// Convert a `RelationshipType` to a valid GQL edge label string.
///
/// Unit variants (e.g. `HasDecision`) are serialized via serde and trimmed of
/// quotes, producing a valid label like `has_decision`. The `Custom(String)`
/// variant returns the inner string directly, avoiding serde's JSON object
/// serialization (`{"Custom":"HAS_BUILD_SYSTEM"}`) which would produce an
/// invalid GQL label atom.
fn relationship_type_to_gql_label(rel_type: &RelationshipType) -> String {
    match rel_type {
        RelationshipType::Custom(s) => s.clone(),
        _ => {
            serde_json::to_string(rel_type)
                .unwrap_or_else(|_| format!("{:?}", rel_type))
                .trim_matches('"')
                .to_string()
        }
    }
}

// ============================================================================

// MemoryGraphMessage Enum — 14 variants matching IMemoryGraph API
// ============================================================================

/// Messages for the MemoryGraph actor.
///
/// This actor is the sole data store for the system, owning graph nodes, edges,
/// and vector embeddings directly (no separate GraphActor or VectorActor).
pub enum MemoryGraphMessage {
    // ── Lifecycle ────────────────────────────────────────
    /// Initialize the graph database with the given data directory.
    /// Creates the GraphDb instance and rebuilds the UUID cache.
    Initialize {
        data_dir: PathBuf,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Initialize the embedding model.
    /// If `embedder` is provided, use it directly (avoids creating a second
    /// CandleEmbedder instance, which does blocking I/O).
    /// If `embedder` is None, creates a new one via `crate::embedder::create_embedder()`.
    InitializeEmbedder {
        model_path: Option<PathBuf>,
        embedder: Option<Arc<dyn Embedder>>,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },

    // ── Node Operations ─────────────────────────────────
    GetNode {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<GraphNode>>>,
    },
    QueryNodes {
        filter: NodeFilter,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    StoreNode {
        node: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    UpdateNode {
        id: String,
        updates: NodeUpdate,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    DeleteNode {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },

    // ── Relationship Operations ──────────────────────────
    CreateRelationship {
        rel: RelationshipInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    GetRelationships {
        node_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphEdge>>>,
    },
    DeleteRelationship {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },

    // ── Traversal ────────────────────────────────────────
    Traverse {
        start_node_id: String,
        options: TraversalOptions,
        reply_to: tokio::sync::oneshot::Sender<Result<TraversalResult>>,
    },

    // ── Context & Memory ─────────────────────────────────
    GetProjectContext {
        reply_to: tokio::sync::oneshot::Sender<Result<ProjectSnapshot>>,
    },
    SearchContext {
        query: String,
        options: Option<SearchOptions>,
        reply_to: tokio::sync::oneshot::Sender<Result<ContextSearchResult>>,
    },
    AddMemory {
        text: String,
        metadata: Option<MemoryMetadata>,
        reply_to: tokio::sync::oneshot::Sender<Result<MemoryEntry>>,
    },
    Recall {
        query: String,
        limit: Option<usize>,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<MemoryEntry>>>,
    },

    // ── Config Storage ───────────────────────────────────
    SetConfig {
        key: String,
        value: serde_json::Value,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    GetConfig {
        key: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<serde_json::Value>>>,
    },

    // ── MCP Config Storage ───────────────────────────────
    /// Bootstrap MCP server config from a JSON file into the graph.
    /// Reads the file, parses it, deletes any existing SpireMcpConfig nodes,
    /// and stores each server as a new SpireMcpConfig node (sync semantics).
    BootstrapMcpConfig {
        config_path: PathBuf,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Get all MCP server config entries from the graph.
    GetMcpConfig {
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<McpServerConfigEntry>>>,
    },

    // ── Batch / Atomic Operations ─────────────────────────
    /// Seed the graph with intent definitions from a JSON file.
    /// Reads the file, parses it, and stores each intent as a SpireNode.
    SeedIntents {
        config_path: PathBuf,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Atomically update a node (delete + insert as one unit).
    /// Safer than the current two-step approach — if the INSERT fails,
    /// the DELETE is rolled back and the node is unchanged.
    AtomicUpdateNode {
        id: String,
        updates: NodeUpdate,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    /// Execute an atomic batch of arbitrary GQL statements.
    BatchGql {
        statements: Vec<String>,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    /// Open a transaction stream for atomic multi-operation batches.
    ///
    /// Returns an `mpsc::Sender<TransactionRequest>` that the caller can use
    /// to push individual operations. Each operation gets a per-op response
    /// via its embedded `oneshot::Sender`. The stream is closed by sending
    /// `StreamOp::Commit` or `StreamOp::Rollback`, or by dropping the sender.
    /// On sender drop without explicit Commit/Rollback, the transaction is
    /// committed automatically (RAII-style, matching `GraphDbTransaction`).
    OpenTransactionStream {
        reply_to: tokio::sync::oneshot::Sender<mpsc::Sender<TransactionRequest>>,
    },


    // ── Maintenance ──────────────────────────────────────
    Sync {
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
}


// ============================================================================
// MemoryGraphActor
// ============================================================================

/// The sole data store actor, backed by SeleneDB's `GraphDb`.
///
/// Owns graph nodes, edges, and vector embeddings via `GraphDb`.
/// All metadata is persisted directly in SeleneDB using GQL statements.
/// No separate GraphActor or VectorActor — all operations are handled inline.
///
/// Enforces schema constraints:
/// - Unique `(type, name)` per node
/// - Referential integrity for relationships (from_id / to_id must exist)
/// - Acyclic `depends_on` relationships
pub struct MemoryGraphActor {
    /// The SeleneDB-backed graph database.
    graph_db: Option<Arc<GraphDb>>,

    /// Embedder for text → vector generation.
    embedder: Option<Arc<dyn Embedder>>,

    /// Data directory for snapshot persistence.
    /// Set during `Initialize` and used by `Sync` to write snapshots.
    data_dir: Option<PathBuf>,

    /// Channel sender for debounced snapshot scheduling.
    /// Each write mutation sends a unit value through this channel;
    /// a background task receives it, debounces for 2 seconds, then
    /// writes a snapshot to disk.
    snapshot_tx: Option<mpsc::UnboundedSender<()>>,

    /// Handle to the background snapshot task, so it can be aborted
    /// on drop if needed.
    snapshot_task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl MemoryGraphActor {
    pub fn new() -> Self {
        let (snapshot_tx, _snapshot_rx) = mpsc::unbounded_channel::<()>();
        Self {
            graph_db: None,
            embedder: None,
            data_dir: None,
            snapshot_tx: Some(snapshot_tx),
            snapshot_task_handle: None,
        }
    }

    /// Initialize the graph database from a data directory.
    ///
    /// Uses `GraphDb::recover()` to restore from the latest snapshot + WAL,
    /// providing cross-session persistence. If no snapshot exists, this falls
    /// back to an empty WAL-backed graph (equivalent to `new_with_wal`).
    ///
    /// If recovery fails (e.g., WAL/snapshot sequence mismatch after a crash),
    /// this method cleans up stale persistence files and starts fresh with a
    /// new WAL-backed graph. This ensures the system can always start even
    /// after a corrupted or inconsistent persistence state.
    fn init_graph(&mut self, data_dir: &PathBuf) -> Result<()> {
        let graph_id = selene_db_core::identity::GraphId::new(1);

        // Attempt recovery from existing snapshot + WAL
        let graph_db = match GraphDb::recover(data_dir, graph_id) {
            Ok(db) => {
                info!("MemoryGraph: graph database recovered from: {}", data_dir.display());
                Arc::new(db)
            }
            Err(e) => {
                let msg = e.to_string();
                warn!("MemoryGraph: recovery failed ({}), attempting clean start", msg);

                // If the error is a WAL/snapshot sequence mismatch, clean up
                // the stale WAL file so recovery can proceed from the snapshot.
                if msg.contains("wal snapshot sequence") && msg.contains("does not match applied snapshot") {
                    warn!("MemoryGraph: WAL/snapshot sequence mismatch detected — removing stale WAL, keeping snapshots");

                    // Remove the live WAL file (created by new_with_wal / recover)
                    let wal_path = data_dir.join("wal.log");
                    if wal_path.exists() {
                        std::fs::remove_file(&wal_path)
                            .map_err(|e| anyhow::anyhow!("Failed to remove stale WAL file: {}", e))?;
                        info!("MemoryGraph: removed stale WAL file: {:?}", wal_path);
                    }

                    // Remove the snapshot-associated WAL file (created by write_snapshot)
                    let spire_wal_path = data_dir.join("spire.wal");
                    if spire_wal_path.exists() {
                        std::fs::remove_file(&spire_wal_path)
                            .map_err(|e| anyhow::anyhow!("Failed to remove stale spire.wal: {}", e))?;
                        info!("MemoryGraph: removed stale spire.wal: {:?}", spire_wal_path);
                    }
                }

                // Fall back to recovering from the directory with stale WAL removed.
                info!("MemoryGraph: recovering graph from: {} (stale WAL removed, snapshots preserved)", data_dir.display());
                let fresh_db = GraphDb::recover(data_dir, graph_id)
                    .map_err(|e| anyhow::anyhow!("Failed to recover fresh graph: {}", e))?;
                Arc::new(fresh_db)
            }
        };

        self.graph_db = Some(graph_db.clone());
        self.data_dir = Some(data_dir.clone());

        // Spawn the background debounced snapshot task.
        let (new_tx, snapshot_rx) = mpsc::unbounded_channel::<()>();
        self.snapshot_tx = Some(new_tx);
        let handle = Self::spawn_snapshot_task(data_dir.clone(), graph_db, snapshot_rx);
        self.snapshot_task_handle = Some(handle);

        Ok(())
    }

    /// Initialize the embedding model.
    fn init_embedder(&mut self, _model_path: Option<PathBuf>, embedder: Option<Arc<dyn Embedder>>) -> Result<()> {
        if let Some(emb) = embedder {
            info!("MemoryGraph: using provided embedder");
            self.embedder = Some(emb);
        } else {
            let emb = crate::embedder::create_embedder()
                .map_err(|e| anyhow::anyhow!("Failed to create embedding model: {}", e))?;
            info!("MemoryGraph: embedding model loaded");
            self.embedder = Some(emb);
        }
        Ok(())
    }

    // ─── GQL Helpers ──────────────────────────────────────────────────────

    /// Escape a string value for use in a GQL literal (single-quoted).
    ///
    /// Escapes both backslashes and single quotes to prevent GQL parse errors
    /// (e.g. "unknown string escape" from regex patterns containing `\[`, `\d`, etc.).
    /// Backslash must be escaped first so that any previously-escaped single quotes
    /// (`\'`) aren't double-escaped.
    fn gql_escape(s: &str) -> String {
        s.replace('\\', "\\\\").replace('\'', "\\'")
    }

    /// Build a GQL property map string from a list of (key, value) pairs.
    /// Values are automatically escaped and quoted as strings.
    fn gql_props(props: &[(&str, &str)]) -> String {
        let parts: Vec<String> = props
            .iter()
            .map(|(k, v)| format!("{}: '{}'", k, Self::gql_escape(v)))
            .collect();
        format!("{{{}}}", parts.join(", "))
    }

    /// Build a GQL property map string from a list of (key, optional_value) pairs.
    /// If the value is None, the property is omitted.
    fn gql_props_opt(props: &[(&str, Option<&str>)]) -> String {
        let parts: Vec<String> = props
            .iter()
            .filter_map(|(k, v)| v.map(|val| format!("{}: '{}'", k, Self::gql_escape(val))))
            .collect();
        format!("{{{}}}", parts.join(", "))
    }

    /// Format a `serde_json::Value` as a native GQL literal.
    ///
    /// This produces type-faithful GQL output:
    /// - Strings → `'escaped value'`
    /// - Numbers → `42` or `3.14`
    /// - Booleans → `true` / `false`
    /// - Null → `null`
    /// - Arrays → `[elem, elem, ...]`
    /// - Objects → `{key: value, key: value, ...}`
    ///
    /// This is used to persist `GraphNode.properties` as native SeleneDB types
    /// rather than JSON-encoded strings, enabling GQL WHERE clauses to work
    /// with proper type semantics (e.g. `WHERE n.count > 10`).
    fn format_value_as_gql(val: &serde_json::Value) -> String {
        match val {
            serde_json::Value::String(s) => format!("'{}'", Self::gql_escape(s)),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".to_string(),
            serde_json::Value::Array(arr) => {
                let items: Vec<String> = arr.iter()
                    .map(|v| Self::format_value_as_gql(v))
                    .collect();
                format!("[{}]", items.join(", "))
            }
            serde_json::Value::Object(obj) => {
                let parts: Vec<String> = obj.iter()
                    .map(|(k, v)| format!("{}: {}", k, Self::format_value_as_gql(v)))
                    .collect();
                format!("{{{}}}", parts.join(", "))
            }
        }
    }

    /// Parse a GQL BindingTable row into a GraphNode.
    /// The row must have a binding named "n" that contains the node properties.
    fn parse_node_from_row(
        row: &selene_db_gql::runtime::Binding,
        table: &selene_db_gql::runtime::BindingTable,
    ) -> Option<GraphNode> {
        use selene_db_core::value::Value;

        // Resolve column index for "n" (the node record)
        let n_idx = table.column_index(crate::graph::to_db_string("n"))?;
        let node_record = row.get(n_idx)?;
        let props = match node_record {
            Value::Record(rec) => match rec.as_ref() {
                selene_db_core::value::Record::Open(fields) => fields,
                _ => return None,
            },
            _ => return None,
        };

        let get_str = |key: &str| -> Option<String> {
            let db_key = selene_db_core::db_string::DbString::try_from(key).ok()?;
            props.iter().find(|(k, _)| k == &db_key).and_then(|(_, v)| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            })
        };

        let get_str_or_empty = |key: &str| -> String {
            get_str(key).unwrap_or_default()
        };

        let uuid = get_str(PROP_UUID)?;
        let node_type_str = get_str(PROP_NODE_TYPE)?;
        let node_type = serde_json::from_str::<NodeType>(&format!("\"{}\"", node_type_str)).unwrap_or(NodeType::Unknown);
        let name = get_str_or_empty(PROP_NAME);
        let description = get_str(PROP_DESCRIPTION);
        let subtype = get_str(PROP_SUBTYPE);
        let embedding_id = get_str(PROP_EMBEDDING_ID);

        let created_at = get_str(PROP_CREATED_AT)
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);
        let updated_at = get_str(PROP_UPDATED_AT)
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);
        let version = {
            let db_key = selene_db_core::db_string::DbString::try_from(PROP_VERSION).ok()?;
            props.iter().find(|(k, _)| k == &db_key).and_then(|(_, v)| {
                if let Value::Int(i) = v { Some(*i as u32) } else { None }
            }).unwrap_or(1)
        };

        // Collect remaining properties (everything except the known keys)
        let known_keys: [&str; 11] = [
            PROP_UUID, PROP_NODE_TYPE, PROP_SUBTYPE, PROP_NAME, PROP_DESCRIPTION,
            PROP_EMBEDDING_ID, PROP_CREATED_AT, PROP_UPDATED_AT, PROP_VERSION,
            "name", "description",
        ];
        let mut properties = HashMap::new();
        for (key, val) in props.iter() {
            let key_str = key.to_string();
            if !known_keys.contains(&key_str.as_str()) {
                if let Some(json_val) = selene_value_to_json(val) {
                    properties.insert(key_str, json_val);
                }
            }
        }

        Some(GraphNode {
            id: uuid,
            node_type,
            subtype,
            name,
            description,
            properties,
            embedding_id,
            created_at,
            updated_at,
            version,
        })
    }

    /// Parse a GQL BindingTable row from a `RETURN n` query into a GraphNode.
    ///
    /// SeleneDB returns a `Value::NodeRef(NodeId)` when projecting the whole node
    /// (i.e. `RETURN n`). This method resolves the `NodeRef` to a full `GraphNode`
    /// by reading all properties (including custom ones) from the SharedGraph
    /// snapshot via `GraphDb::resolve_node_properties`.
    fn parse_node_from_ref_row(
        row: &selene_db_gql::runtime::Binding,
        table: &selene_db_gql::runtime::BindingTable,
        graph_db: &GraphDb,
    ) -> Option<GraphNode> {
        use selene_db_core::value::Value;

        // Resolve column index for "n" (the node reference)
        let n_idx = table.column_index(crate::graph::to_db_string("n"))?;
        let node_ref = row.get(n_idx)?;

        // Extract NodeId from Value::NodeRef
        let node_id = match node_ref {
            Value::NodeRef(nid) => *nid,
            _ => return None,
        };

        // Resolve all properties from the SharedGraph snapshot
        let props = graph_db.resolve_node_properties(node_id)?;

        // Helper to get a string property from the PropertyMap
        let get_str = |key: &str| -> Option<String> {
            let db_key = selene_db_core::db_string::DbString::try_from(key).ok()?;
            props.get(&db_key).and_then(|v| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            })
        };

        let get_str_or_empty = |key: &str| -> String {
            get_str(key).unwrap_or_default()
        };

        let uuid = get_str(PROP_UUID)?;
        let node_type_str = get_str(PROP_NODE_TYPE)?;
        let node_type = serde_json::from_str::<NodeType>(&format!("\"{}\"", node_type_str)).unwrap_or(NodeType::Unknown);
        let name = get_str_or_empty(PROP_NAME);
        let description = get_str(PROP_DESCRIPTION);
        let subtype = get_str(PROP_SUBTYPE);
        let embedding_id = get_str(PROP_EMBEDDING_ID);

        let created_at = get_str(PROP_CREATED_AT)
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);
        let updated_at = get_str(PROP_UPDATED_AT)
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);
        let version = {
            let db_key = selene_db_core::db_string::DbString::try_from(PROP_VERSION).ok()?;
            props.get(&db_key).and_then(|v| {
                if let Value::Int(i) = v { Some(*i as u32) } else { None }
            }).unwrap_or(1)
        };

        // Collect remaining properties (everything except the known keys)
        let known_keys: [&str; 9] = [
            PROP_UUID, PROP_NODE_TYPE, PROP_SUBTYPE, PROP_NAME, PROP_DESCRIPTION,
            PROP_EMBEDDING_ID, PROP_CREATED_AT, PROP_UPDATED_AT, PROP_VERSION,
        ];
        let mut properties = HashMap::new();
        for (key, val) in props.iter() {
            let key_str = key.to_string();
            if !known_keys.contains(&key_str.as_str()) {
                if let Some(json_val) = selene_value_to_json(val) {
                    properties.insert(key_str, json_val);
                }
            }
        }

        Some(GraphNode {
            id: uuid,
            node_type,
            subtype,
            name,
            description,
            properties,
            embedding_id,
            created_at,
            updated_at,
            version,
        })
    }

    /// Parse a GQL BindingTable row into a GraphEdge.
    /// The row must have bindings "e" (edge), "from_uuid", and "to_uuid".
    fn parse_edge_from_row(
        row: &selene_db_gql::runtime::Binding,
        table: &selene_db_gql::runtime::BindingTable,
    ) -> Option<GraphEdge> {
        use selene_db_core::value::Value;

        // Resolve column indices
        let e_idx = table.column_index(crate::graph::to_db_string("e"))?;
        let from_idx = table.column_index(crate::graph::to_db_string("from_uuid"))?;
        let to_idx = table.column_index(crate::graph::to_db_string("to_uuid"))?;

        let edge_record = row.get(e_idx)?;
        let props = match edge_record {
            Value::Record(rec) => match rec.as_ref() {
                selene_db_core::value::Record::Open(fields) => fields,
                _ => return None,
            },
            _ => return None,
        };

        let get_str = |key: &str| -> Option<String> {
            let db_key = selene_db_core::db_string::DbString::try_from(key).ok()?;
            props.iter().find(|(k, _)| k == &db_key).and_then(|(_, v)| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            })
        };

        let uuid = get_str(PROP_UUID)?;
        let edge_type_str = get_str(PROP_EDGE_TYPE)?;
        let edge_type = serde_json::from_str::<RelationshipType>(&format!("\"{}\"", edge_type_str)).unwrap_or(RelationshipType::Unknown);

        // Get from/to UUIDs from the row bindings
        let from_uuid = row.get(from_idx).and_then(|v| {
            if let Value::String(s) = v { Some(s.to_string()) } else { None }
        }).unwrap_or_default();
        let to_uuid = row.get(to_idx).and_then(|v| {
            if let Value::String(s) = v { Some(s.to_string()) } else { None }
        }).unwrap_or_default();

        let weight = {
            let db_key = selene_db_core::db_string::DbString::try_from(PROP_WEIGHT).ok()?;
            props.iter().find(|(k, _)| k == &db_key).and_then(|(_, v)| {
                if let Value::Float(f) = v { Some(*f) }
                else if let Value::Int(i) = v { Some(*i as f64) }
                else { None }
            })
        };

        let created_at = get_str(PROP_CREATED_AT)
            .and_then(|s| s.parse::<DateTime<Utc>>().ok())
            .unwrap_or_else(Utc::now);

        // Collect remaining properties
        let known_keys: [&str; 4] = [PROP_UUID, PROP_EDGE_TYPE, PROP_WEIGHT, PROP_CREATED_AT];
        let mut properties = HashMap::new();
        for (key, val) in props.iter() {
            let key_str = key.to_string();
            if !known_keys.contains(&key_str.as_str()) {
                if let Some(json_val) = selene_value_to_json(val) {
                    properties.insert(key_str, json_val);
                }
            }
        }

        Some(GraphEdge {
            id: uuid,
            edge_type,
            from_id: from_uuid,
            to_id: to_uuid,
            properties,
            created_at,
            weight,
        })
    }

    /// Query all SpireNode-labeled nodes and return them as GraphNodes.
    fn query_all_spire_nodes(&self) -> Vec<GraphNode> {
        let graph_db = match self.graph_db.as_ref() {
            Some(db) => db,
            None => return Vec::new(),
        };

        // Use RETURN n to get the full node including all custom properties.
        // SeleneDB returns a Value::NodeRef for RETURN n, which we resolve
        // via GraphDb::resolve_node_properties to read all properties.
        let gql = format!(
            "MATCH (n:{}) RETURN n",
            LABEL_SPIRE_NODE,
        );
        let table = match graph_db.execute_gql_query(&gql) {
            Ok(Some(t)) => t,
            _ => return Vec::new(),
        };

        table.rows().iter()
            .filter_map(|row| Self::parse_node_from_ref_row(row, &table, graph_db))
            .collect()
    }

    /// Query a single SpireNode by UUID.
    fn query_node_by_uuid(&self, uuid: &str) -> Option<GraphNode> {
        let graph_db = self.graph_db.as_ref()?;
        // Use RETURN n to get the full node including all custom properties.
        let gql = format!(
            "MATCH (n:{}) WHERE n.uuid = '{}' RETURN n",
            LABEL_SPIRE_NODE,
            Self::gql_escape(uuid),
        );
        let table = graph_db.execute_gql_query(&gql).ok()??;
        table.rows().first()
            .and_then(|row| Self::parse_node_from_ref_row(row, &table, graph_db))
    }

    /// Query edges for a node (both outgoing and incoming) via GQL.
    fn query_edges_for_node(&self, node_uuid: &str) -> Vec<GraphEdge> {
        let graph_db = match self.graph_db.as_ref() {
            Some(db) => db,
            None => return Vec::new(),
        };

        let mut edges = Vec::new();

        // Outgoing edges
        let outgoing_gql = format!(
            "MATCH (n:{})-[e]->(m) WHERE n.uuid = '{}' RETURN e, n.uuid AS from_uuid, m.uuid AS to_uuid",
            LABEL_SPIRE_NODE,
            Self::gql_escape(node_uuid)
        );
        if let Ok(Some(table)) = graph_db.execute_gql_query(&outgoing_gql) {
            for row in table.rows() {
                if let Some(edge) = Self::parse_edge_from_row(row, &table) {
                    edges.push(edge);
                }
            }
        }

        // Incoming edges
        let incoming_gql = format!(
            "MATCH (m)-[e]->(n:{}) WHERE n.uuid = '{}' RETURN e, m.uuid AS from_uuid, n.uuid AS to_uuid",
            LABEL_SPIRE_NODE,
            Self::gql_escape(node_uuid)
        );
        if let Ok(Some(table)) = graph_db.execute_gql_query(&incoming_gql) {
            for row in table.rows() {
                if let Some(edge) = Self::parse_edge_from_row(row, &table) {
                    edges.push(edge);
                }
            }
        }

        edges
    }

    /// Check whether a node with the given `(type, name)` already exists.
    fn has_duplicate(&self, node_type: &NodeType, name: &str) -> bool {
        let graph_db = match self.graph_db.as_ref() {
            Some(db) => db,
            None => return false,
        };

        let type_str = serde_json::to_string(node_type)
            .unwrap_or_else(|_| format!("{:?}", node_type))
            .trim_matches('"')
            .to_string();

        let gql = format!(
            "MATCH (n:{}) WHERE n.{} = '{}' AND n.{} = '{}' RETURN n LIMIT 1",
            LABEL_SPIRE_NODE,
            PROP_NODE_TYPE,
            Self::gql_escape(&type_str),
            PROP_NAME,
            Self::gql_escape(name),
        );

        match graph_db.execute_gql_query(&gql) {
            Ok(Some(table)) => {
                if table.rows().is_empty() {
                    return false;
                }
                // For Unknown-type nodes (files/directories), skip the simple check
                if *node_type == NodeType::Unknown {
                    return false;
                }
                true
            }
            _ => false,
        }
    }

    /// Convert a `NodeInput` into a fully populated `GraphNode`.
    fn create_node_from_input(input: NodeInput) -> GraphNode {
        let now = Utc::now();
        GraphNode {
            id: Uuid::new_v4().to_string(),
            node_type: input.node_type,
            subtype: input.subtype,
            name: input.name,
            description: input.description,
            properties: input.properties.unwrap_or_default(),
            embedding_id: input.embedding_id,
            created_at: now,
            updated_at: now,
            version: 1,
        }
    }

    /// Apply partial updates to an existing node.
    fn apply_updates(node: &GraphNode, updates: NodeUpdate) -> GraphNode {
        let mut updated = node.clone();
        if let Some(v) = updates.node_type {
            updated.node_type = v;
        }
        if let Some(v) = updates.subtype {
            updated.subtype = v;
        }
        if let Some(v) = updates.name {
            updated.name = v;
        }
        if let Some(v) = updates.description {
            updated.description = v;
        }
        if let Some(v) = updates.properties {
            updated.properties = v;
        }
        if let Some(v) = updates.embedding_id {
            updated.embedding_id = v;
        }
        updated.updated_at = Utc::now();
        updated.version += 1;
        updated
    }

    /// Store a node in SeleneDB via GQL INSERT and return the allocated UUID string.
    ///
    /// If `embedding_vector` is provided, it is stored as a vector property named
    /// `"embedding"` on the node, enabling vector search via `exact_vector_search_nodes`.
    fn store_node_via_gql(&self, graph_node: &GraphNode, embedding_vector: Option<&[f32]>) -> Result<()> {
        let graph_db = self.graph_db.as_ref().ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let node_type_json = serde_json::to_string(&graph_node.node_type)
            .unwrap_or_else(|_| format!("{:?}", graph_node.node_type));
        let node_type_stored = node_type_json.trim_matches('"').to_string();

        let description_val = graph_node.description.as_deref().unwrap_or("");
        let subtype_val = graph_node.subtype.as_deref().unwrap_or("");
        let embedding_id_val = graph_node.embedding_id.as_deref().unwrap_or("");

        // Use a single label (SpireNode) for INSERT compatibility.
        // Node type and subtype are stored as properties for querying.
        let created_at_str = graph_node.created_at.to_rfc3339();
        let updated_at_str = graph_node.updated_at.to_rfc3339();
        let version_str = graph_node.version.to_string();

        // Serialize custom properties as native GQL types
        let mut extra_parts: Vec<String> = Vec::new();
        for (key, val) in &graph_node.properties {
            extra_parts.push(format!("{}: {}", key, Self::format_value_as_gql(val)));
        }

        let props = [
            (PROP_UUID, graph_node.id.as_str()),
            (PROP_NODE_TYPE, node_type_stored.as_str()),
            (PROP_NAME, graph_node.name.as_str()),
            (PROP_DESCRIPTION, description_val),
            (PROP_SUBTYPE, subtype_val),
            (PROP_EMBEDDING_ID, embedding_id_val),
            (PROP_CREATED_AT, created_at_str.as_str()),
            (PROP_UPDATED_AT, updated_at_str.as_str()),
            (PROP_VERSION, version_str.as_str()),
        ];

        // Build the GQL property map string
        let mut parts: Vec<String> = props.iter()
            .map(|(k, v)| format!("{}: '{}'", k, Self::gql_escape(v)))
            .collect();
        parts.extend(extra_parts);

        // If an embedding vector is provided, add it as a vector property
        let gql = if let Some(embedding) = embedding_vector {
            let vec_str: Vec<String> = embedding.iter().map(|v| v.to_string()).collect();
            let vec_list = format!("[{}]", vec_str.join(", "));
            // We need to use a different approach: first create the node with scalar props,
            // then SET the embedding vector property separately since GQL INSERT doesn't
            // support vector literals in property maps.
            let props_str = format!("{{{}}}", parts.join(", "));
            let create_gql = format!("INSERT (n:{} {})", LABEL_SPIRE_NODE, props_str);
            graph_db.execute_gql_write(&create_gql)?;

            // Now SET the embedding vector property
            let set_gql = format!(
                "MATCH (n) WHERE n.{} = '{}' SET n.embedding = {}",
                PROP_UUID,
                Self::gql_escape(&graph_node.id),
                vec_list,
            );
            graph_db.execute_gql_write(&set_gql)?;
            return Ok(());
        } else {
            let props_str = format!("{{{}}}", parts.join(", "));
            format!("INSERT (n:{} {})", LABEL_SPIRE_NODE, props_str)
        };

        graph_db.execute_gql_write(&gql)?;
        Ok(())
    }

    /// Store an edge in SeleneDB via GQL.
    fn store_edge_via_gql(
        &self,
        from_uuid: &str,
        predicate: &str,
        to_uuid: &str,
        properties: &[(&str, &str)],
    ) -> Result<()> {
        let graph_db = self.graph_db.as_ref().ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let props_str = Self::gql_props(properties);

        let gql = format!(
            "MATCH (a), (b) WHERE a.uuid = '{}' AND b.uuid = '{}' INSERT (a)-[e:{} {}]->(b)",
            Self::gql_escape(from_uuid),
            Self::gql_escape(to_uuid),
            predicate,
            props_str,
        );
        graph_db.execute_gql_write(&gql)?;
        Ok(())
    }

    /// Delete a node and all its edges via GQL.
    fn delete_node_via_gql(&self, uuid: &str) -> Result<()> {
        let graph_db = self.graph_db.as_ref().ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let gql = format!(
            "MATCH (n) WHERE n.uuid = '{}' DETACH DELETE n",
            Self::gql_escape(uuid),
        );
        graph_db.execute_gql_write(&gql)?;
        Ok(())
    }

    /// Delete an edge by UUID via GQL.
    fn delete_edge_via_gql(&self, uuid: &str) -> Result<()> {
        let graph_db = self.graph_db.as_ref().ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let gql = format!(
            "MATCH ()-[e]->() WHERE e.uuid = '{}' DELETE e",
            Self::gql_escape(uuid),
        );
        graph_db.execute_gql_write(&gql)?;
        Ok(())
    }

    /// Query nodes by filter using GQL.
    fn query_nodes(&self, filter: NodeFilter) -> Vec<GraphNode> {
        let graph_db = match self.graph_db.as_ref() {
            Some(db) => db,
            None => return Vec::new(),
        };

        // Build WHERE clauses
        let mut conditions: Vec<String> = Vec::new();

        if let Some(ref nt) = filter.node_type {
            let type_str = serde_json::to_string(nt)
                .unwrap_or_else(|_| format!("{:?}", nt))
                .trim_matches('"')
                .to_string();
            conditions.push(format!("n.{} = '{}'", PROP_NODE_TYPE, Self::gql_escape(&type_str)));
        }
        if let Some(ref st) = filter.subtype {
            conditions.push(format!("n.{} = '{}'", PROP_SUBTYPE, Self::gql_escape(st)));
        }
        if let Some(ref name) = filter.name {
            conditions.push(format!("n.{} = '{}'", PROP_NAME, Self::gql_escape(name)));
        }
        if let Some(ref status) = filter.status {
            conditions.push(format!("n.status = '{}'", Self::gql_escape(status)));
        }
        if let Some(ref tags) = filter.tags {
            for tag in tags {
                conditions.push(format!("'{}' IN n.tags", Self::gql_escape(tag)));
            }
        }
        if let Some(ref props) = filter.properties {
            for (key, value) in props {
                let val_str = match value {
                    serde_json::Value::String(s) => Self::gql_escape(s),
                    serde_json::Value::Number(n) => n.to_string(),
                    serde_json::Value::Bool(b) => b.to_string(),
                    _ => Self::gql_escape(&value.to_string()),
                };
                conditions.push(format!("n.{} = '{}'", Self::gql_escape(key), val_str));
            }
        }

        let where_clause = if conditions.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", conditions.join(" AND "))
        };

        let limit_clause = filter.limit.map(|l| format!(" LIMIT {}", l)).unwrap_or_default();
        let offset_clause = filter.offset.map(|o| format!(" SKIP {}", o)).unwrap_or_default();

        // Use RETURN n to get the full node including all custom properties.
        let gql = format!(
            "MATCH (n:{}){} RETURN n{}{}",
            LABEL_SPIRE_NODE,
            where_clause,
            limit_clause,
            offset_clause,
        );

        match graph_db.execute_gql_query(&gql) {
            Ok(Some(table)) => {
                table.rows().iter()
                    .filter_map(|row| Self::parse_node_from_ref_row(row, &table, graph_db))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Check whether adding a `depends_on` edge from `from_id` to `to_id`
    /// would create a cycle. Uses GQL-based traversal.
    fn would_create_cycle(&self, from_id: &str, to_id: &str) -> bool {
        if from_id == to_id {
            return true;
        }

        let graph_db = match self.graph_db.as_ref() {
            Some(db) => db,
            None => return false,
        };

        // GQL: check if there's a path from to_id back to from_id via depends_on edges
        let gql = format!(
            "MATCH (a)-[e:DependsOn*]->(b) WHERE a.uuid = '{}' AND b.uuid = '{}' RETURN a, b LIMIT 1",
            Self::gql_escape(to_id),
            Self::gql_escape(from_id),
        );

        match graph_db.execute_gql_query(&gql) {
            Ok(Some(table)) => !table.rows().is_empty(),
            _ => false,
        }
    }

    /// BFS traversal from a start node using GQL.
    fn traverse(
        &self,
        start_node_id: &str,
        options: &TraversalOptions,
    ) -> Result<TraversalResult> {
        let graph_db = self.graph_db.as_ref().ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let direction = &options.direction;
        let max_depth = options.max_depth;
        let edge_types = &options.relationship_types;

        // Build the GQL variable-length path pattern based on direction
        let arrow = match direction {
            Some(TraversalDirection::Out) => "->",
            Some(TraversalDirection::In) => "<-",
            Some(TraversalDirection::Both) => "-",
            None => "-",
        };

        // Build edge type filter
        let edge_pattern = if edge_types.as_ref().map_or(true, |v| v.is_empty()) {
            format!("[e*1..{}]{}", max_depth, arrow)
        } else {
            let types: Vec<String> = edge_types.as_ref().unwrap().iter()
                .map(|t| format!(":{:?}", t))
                .collect();
            format!("[e*1..{}{}]{}", max_depth, types.join("|"), arrow)
        };

        let gql = format!(
            "MATCH path = (start:{}){}(end) WHERE start.uuid = '{}' RETURN path, start.uuid AS from_uuid, end.uuid AS to_uuid",
            LABEL_SPIRE_NODE,
            edge_pattern,
            Self::gql_escape(start_node_id),
        );

        let table = match graph_db.execute_gql_query(&gql) {
            Ok(Some(t)) => t,
            _ => return Ok(TraversalResult {
                nodes: Vec::new(),
                edges: Vec::new(),
                paths: Vec::new(),
            }),
        };

        let mut nodes: HashMap<String, GraphNode> = HashMap::new();
        let mut edges: Vec<GraphEdge> = Vec::new();
        let mut paths: Vec<TraversalPath> = Vec::new();

        for row in table.rows() {
            // Parse the start node
            if let Some(node) = Self::parse_node_from_row(row, &table) {
                nodes.entry(node.id.clone()).or_insert(node);
            }

            // Parse the end node
            if let Some(end_idx) = table.column_index(crate::graph::to_db_string("end")) {
                if let Some(end_record) = row.get(end_idx) {
                    let end_uuid = match end_record {
                        Value::Record(rec) => {
                            match rec.as_ref() {
                                selene_db_core::value::Record::Open(fields) => {
                                    let db_key = match selene_db_core::db_string::DbString::try_from(PROP_UUID) {
                                        Ok(k) => k,
                                        Err(_) => return Ok(TraversalResult {
                                            nodes: nodes.into_values().collect(),
                                            edges,
                                            paths,
                                        }),
                                    };
                                    fields.iter().find(|(k, _)| k == &db_key).and_then(|(_, v)| {
                                        if let Value::String(s) = v { Some(s.to_string()) } else { None }
                                    })
                                }
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(uuid) = end_uuid {
                        if let Some(node) = self.query_node_by_uuid(&uuid) {
                            nodes.entry(node.id.clone()).or_insert(node);
                        }
                    }
                }
            }

            // Parse edge from the row
            if let Some(edge) = Self::parse_edge_from_row(row, &table) {
                if !edges.iter().any(|e| e.id == edge.id) {
                    edges.push(edge);
                }
            }
        }

        Ok(TraversalResult {
            nodes: nodes.into_values().collect(),
            edges,
            paths,
        })
    }

    /// Find a node by (node_type, name) within an existing transaction.
    /// Returns `None` if no matching node exists.
    fn find_node_by_type_and_name(
        txn: &mut crate::graph::GraphDbTransaction,
        node_type: &NodeType,
        name: &str,
    ) -> anyhow::Result<Option<GraphNode>> {
        let type_str = serde_json::to_string(node_type)
            .unwrap_or_else(|_| format!("{:?}", node_type))
            .trim_matches('"')
            .to_string();

        let gql = format!(
            "MATCH (n:{}) WHERE n.{} = '{}' AND n.{} = '{}' RETURN n LIMIT 1",
            LABEL_SPIRE_NODE,
            PROP_NODE_TYPE,
            Self::gql_escape(&type_str),
            PROP_NAME,
            Self::gql_escape(name),
        );

        let table = txn.execute_gql(&gql)?;
        match table {
            StatementOutput::Rows(table) => {
                if let Some(row) = table.rows().first() {
                    // Use parse_node_from_row since the transaction returns Value::Record
                    // (not Value::NodeRef) for RETURN n within a transaction context.
                    Ok(Self::parse_node_from_row(row, &table))
                } else {
                    Ok(None)
                }
            }
            _ => Ok(None),
        }
    }

    /// Schedule a debounced snapshot write.
    fn schedule_snapshot(&self) {
        if let Some(tx) = &self.snapshot_tx {
            let _ = tx.send(());
        }
    }

    /// Spawn the background debounced snapshot task.
    fn spawn_snapshot_task(
        data_dir: PathBuf,
        graph_db: Arc<GraphDb>,
        mut snapshot_rx: mpsc::UnboundedReceiver<()>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let debounce_duration = Duration::from_secs(2);

            loop {
                // Wait for the first signal
                match snapshot_rx.recv().await {
                    Some(_) => {
                        // Debounce: wait for more signals, resetting the timer each time
                        loop {
                            tokio::select! {
                                Some(_) = snapshot_rx.recv() => {
                                    // Another mutation came in — reset the debounce timer
                                    continue;
                                }
                                _ = tokio::time::sleep(debounce_duration) => {
                                    // No more mutations within the debounce window — write snapshot
                                    break;
                                }
                            }
                        }

                        // Write snapshot
                        let next_seq = match GraphDb::latest_snapshot_sequence(&data_dir) {
                            Ok(Some(seq)) => seq + 1,
                            Ok(None) => 1,
                            Err(e) => {
                                warn!("MemoryGraph snapshot task: failed to get latest sequence: {}", e);
                                continue;
                            }
                        };

                        match graph_db.write_snapshot(&data_dir, next_seq, true) {
                            Ok(outcome) => {
                                info!(
                                    "MemoryGraph snapshot task: snapshot written (seq={}, sections={})",
                                    outcome.snapshot_seq,
                                    outcome.section_count,
                                );
                            }
                            Err(e) => {
                                warn!("MemoryGraph snapshot task: failed to write snapshot: {}", e);
                            }
                        }
                    }
                    None => {
                        // Channel closed — shut down
                        info!("MemoryGraph snapshot task: shutting down");
                        break;
                    }
                }
            }
        })
    }

    // ─── Async Handlers (embedding requires async) ─────────────────────────

    /// Handle `SearchContext` — embed query, vector search, return scored nodes.
    async fn handle_search_context(
        &self,
        query: String,
        options: Option<SearchOptions>,
    ) -> Result<ContextSearchResult> {
        let embedder = self.embedder.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized"))?;
        let graph_db = self.graph_db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        // Generate embedding for the query
        let embedding = embedder.embed(&query).await?;

        // Perform vector search
        let k = options.as_ref().and_then(|o| o.top_k).unwrap_or(10);
        let hits = graph_db.exact_vector_search_nodes(
            LABEL_SPIRE_NODE,
            "embedding",
            &embedding.vector,
            VectorMetric::Cosine,
            k,
        )?;

        let mut scored_nodes = Vec::new();
        for hit in &hits {
            // Get the node UUID from the hit's node_id
            let gql = format!(
                "MATCH (n) WHERE id(n) = {} RETURN n.uuid",
                hit.node_id.get()
            );
            if let Ok(Some(table)) = graph_db.execute_gql_query(&gql) {
                if let Some(row) = table.rows().first() {
                    if let Some(uuid_idx) = table.column_index(crate::graph::to_db_string("n.uuid")) {
                        if let Some(Value::String(uuid)) = row.get(uuid_idx) {
                            if let Some(node) = self.query_node_by_uuid(&uuid.to_string()) {
                                scored_nodes.push(ScoredNode {
                                    node,
                                    similarity: hit.distance,
                                    source: RetrievalSource::Semantic,
                                    score: hit.distance,
                                });
                            }
                        }
                    }
                }
            }
        }

        let total = scored_nodes.len();
        Ok(ContextSearchResult {
            nodes: scored_nodes,
            relationships: Vec::new(),
            total_results: total,
            search_time_ms: 0,
            truncated: false,
        })
    }

    /// Handle `AddMemory` — embed text, store as node, return entry.
    async fn handle_add_memory(
        &self,
        text: String,
        metadata: Option<MemoryMetadata>,
    ) -> Result<MemoryEntry> {
        let embedder = self.embedder.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized"))?;
        let now = Utc::now();
        let memory_id = Uuid::new_v4().to_string();

        // Generate embedding
        let embedding = embedder.embed(&text).await?;

        // Store as a node
        let mem_type_str = metadata.as_ref()
            .and_then(|m| m.mem_type.as_ref())
            .map(|t| format!("{:?}", t));
        let graph_node = GraphNode {
            id: memory_id.clone(),
            node_type: NodeType::Unknown,
            subtype: mem_type_str,
            name: text.chars().take(100).collect(),
            description: Some(text.clone()),
            properties: HashMap::new(),
            embedding_id: Some(memory_id.clone()),
            created_at: now,
            updated_at: now,
            version: 1,
        };

        self.store_node_via_gql(&graph_node, Some(&embedding.vector))?;
        self.schedule_snapshot();

        let default_metadata = MemoryMetadata {
            mem_type: None,
            tags: None,
            source: None,
            confidence: None,
        };
        let mem_id = memory_id.clone();
        Ok(MemoryEntry {
            id: mem_id,
            text,
            metadata: metadata.unwrap_or(default_metadata),
            embedding_id: memory_id.clone(),
            node_id: Some(memory_id),
            created_at: now,
            updated_at: now,
        })
    }

    /// Handle `Recall` — embed query, vector search, return memory entries.
    async fn handle_recall(
        &self,
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<MemoryEntry>> {
        let embedder = self.embedder.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized"))?;
        let graph_db = self.graph_db.as_ref()
            .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

        let embedding = embedder.embed(&query).await?;
        let k = limit.unwrap_or(10);

        let hits = graph_db.exact_vector_search_nodes(
            LABEL_SPIRE_NODE,
            "embedding",
            &embedding.vector,
            VectorMetric::Cosine,
            k,
        )?;

        let mut entries = Vec::new();
        for hit in &hits {
            // Use RETURN n to get the full node including all custom properties.
            let gql = format!(
                "MATCH (n) WHERE id(n) = {} RETURN n",
                hit.node_id.get(),
            );
            if let Ok(Some(table)) = graph_db.execute_gql_query(&gql) {
                if let Some(row) = table.rows().first() {
                    if let Some(node) = Self::parse_node_from_ref_row(row, &table, graph_db) {
                        let default_metadata = MemoryMetadata {
                            mem_type: None,
                            tags: None,
                            source: None,
                            confidence: None,
                        };
                        let node_id = node.id.clone();
                        entries.push(MemoryEntry {
                            id: node_id,
                            text: node.description.unwrap_or(node.name),
                            metadata: default_metadata,
                            embedding_id: node.embedding_id.unwrap_or_default(),
                            node_id: Some(node.id.clone()),
                            created_at: node.created_at,
                            updated_at: node.updated_at,
                        });
                    }
                }
            }
        }

        Ok(entries)
    }

    /// Store a node inside an existing transaction (GQL INSERT + optional embedding SET).
    fn store_node_in_txn(
        txn: &mut crate::graph::GraphDbTransaction,
        graph_node: &GraphNode,
        embedding_vector: Option<&[f32]>,
    ) -> anyhow::Result<()> {
        let node_type_json = serde_json::to_string(&graph_node.node_type)
            .unwrap_or_else(|_| format!("{:?}", graph_node.node_type));
        let node_type_stored = node_type_json.trim_matches('"').to_string();

        let description_val = graph_node.description.as_deref().unwrap_or("");
        let subtype_val = graph_node.subtype.as_deref().unwrap_or("");
        let embedding_id_val = graph_node.embedding_id.as_deref().unwrap_or("");

        let created_at_str = graph_node.created_at.to_rfc3339();
        let updated_at_str = graph_node.updated_at.to_rfc3339();
        let version_str = graph_node.version.to_string();

        // Serialize custom properties as native GQL types
        let mut extra_parts: Vec<String> = Vec::new();
        for (key, val) in &graph_node.properties {
            extra_parts.push(format!("{}: {}", key, Self::format_value_as_gql(val)));
        }

        let props = [
            (PROP_UUID, graph_node.id.as_str()),
            (PROP_NODE_TYPE, node_type_stored.as_str()),
            (PROP_NAME, graph_node.name.as_str()),
            (PROP_DESCRIPTION, description_val),
            (PROP_SUBTYPE, subtype_val),
            (PROP_EMBEDDING_ID, embedding_id_val),
            (PROP_CREATED_AT, created_at_str.as_str()),
            (PROP_UPDATED_AT, updated_at_str.as_str()),
            (PROP_VERSION, version_str.as_str()),
        ];

        let mut parts: Vec<String> = props.iter()
            .map(|(k, v)| format!("{}: '{}'", k, Self::gql_escape(v)))
            .collect();
        parts.extend(extra_parts);
        let props_str = format!("{{{}}}", parts.join(", "));
        let gql = format!("INSERT (n:{} {})", LABEL_SPIRE_NODE, props_str);
        txn.execute_gql_write(&gql)?;

        // If an embedding vector is provided, SET it on the node after INSERT
        if let Some(embedding) = embedding_vector {
            let vec_str: Vec<String> = embedding.iter().map(|v| v.to_string()).collect();
            let vec_list = format!("[{}]", vec_str.join(", "));
            let set_gql = format!(
                "MATCH (n) WHERE n.{} = '{}' SET n.embedding = {}",
                PROP_UUID,
                Self::gql_escape(&graph_node.id),
                vec_list,
            );
            txn.execute_gql_write(&set_gql)?;
        }

        Ok(())
    }

    /// Execute a single `StreamOp` within an existing transaction.
    ///
    /// This is the core dispatch for the transaction stream. Each operation
    /// is executed against the provided `GraphDbTransaction` and returns a
    /// `StreamOpResult` on success.
    fn execute_stream_op_in_txn(
        txn: &mut crate::graph::GraphDbTransaction,
        op: &StreamOp,
    ) -> anyhow::Result<StreamOpResult> {
        match op {
            StreamOp::StoreNode(input) => {
                let graph_node = Self::create_node_from_input(input.clone());
                Self::store_node_in_txn(txn, &graph_node, None)?;
                Ok(StreamOpResult::NodeStored(graph_node))
            }
            StreamOp::StoreNodeWithEmbedding { node, embedding_vector } => {
                let graph_node = Self::create_node_from_input(node.clone());
                Self::store_node_in_txn(txn, &graph_node, Some(embedding_vector))?;
                Ok(StreamOpResult::NodeStored(graph_node))
            }
            StreamOp::UpdateNode { id, updates } => {
                // We can't query within a transaction easily, so we use a raw GQL SET approach
                // Delete old and insert new within the transaction
                let delete_gql = format!(
                    "MATCH (n) WHERE n.uuid = '{}' DETACH DELETE n",
                    Self::gql_escape(id),
                );
                txn.execute_gql_write(&delete_gql)?;

                // Reconstruct the node from the update
                let now = Utc::now();
                let updated_node = GraphNode {
                    id: id.clone(),
                    node_type: updates.node_type.clone().unwrap_or(NodeType::Unknown),
                    subtype: updates.subtype.clone().unwrap_or(None),
                    name: updates.name.clone().unwrap_or_default(),
                    description: updates.description.clone().unwrap_or(None),
                    properties: updates.properties.clone().unwrap_or_default(),
                    embedding_id: updates.embedding_id.clone().unwrap_or(None),
                    created_at: now,
                    updated_at: now,
                    version: 1,
                };

                let node_type_json = serde_json::to_string(&updated_node.node_type)
                    .unwrap_or_else(|_| format!("{:?}", updated_node.node_type));
                let node_type_stored = node_type_json.trim_matches('"').to_string();

                let description_val = updated_node.description.as_deref().unwrap_or("");
                let subtype_val = updated_node.subtype.as_deref().unwrap_or("");
                let embedding_id_val = updated_node.embedding_id.as_deref().unwrap_or("");

                let created_at_str = updated_node.created_at.to_rfc3339();
                let updated_at_str = updated_node.updated_at.to_rfc3339();
                let version_str = updated_node.version.to_string();

                // Serialize custom properties as native GQL types
                let mut extra_parts: Vec<String> = Vec::new();
                for (key, val) in &updated_node.properties {
                    extra_parts.push(format!("{}: {}", key, Self::format_value_as_gql(val)));
                }

                let props = [
                    (PROP_UUID, updated_node.id.as_str()),
                    (PROP_NODE_TYPE, node_type_stored.as_str()),
                    (PROP_NAME, updated_node.name.as_str()),
                    (PROP_DESCRIPTION, description_val),
                    (PROP_SUBTYPE, subtype_val),
                    (PROP_EMBEDDING_ID, embedding_id_val),
                    (PROP_CREATED_AT, created_at_str.as_str()),
                    (PROP_UPDATED_AT, updated_at_str.as_str()),
                    (PROP_VERSION, version_str.as_str()),
                ];

                let mut parts: Vec<String> = props.iter()
                    .map(|(k, v)| format!("{}: '{}'", k, Self::gql_escape(v)))
                    .collect();
                parts.extend(extra_parts);
                let props_str = format!("{{{}}}", parts.join(", "));
                let gql = format!("INSERT (n:{} {})", LABEL_SPIRE_NODE, props_str);
                txn.execute_gql_write(&gql)?;
                Ok(StreamOpResult::NodeUpdated(updated_node))
            }
            StreamOp::DeleteNode(id) => {
                let gql = format!(
                    "MATCH (n) WHERE n.uuid = '{}' DETACH DELETE n",
                    Self::gql_escape(id),
                );
                txn.execute_gql_write(&gql)?;
                Ok(StreamOpResult::NodeDeleted)
            }
            StreamOp::CreateRelationship(rel) => {
                let edge_uuid = Uuid::new_v4().to_string();
                let now = Utc::now();
                let edge_type_stored = relationship_type_to_gql_label(&rel.edge_type);

                let created_at_str = now.to_rfc3339();
                let mut edge_props: Vec<(&str, &str)> = vec![
                    (PROP_UUID, &edge_uuid),
                    (PROP_EDGE_TYPE, &edge_type_stored),
                    (PROP_CREATED_AT, &created_at_str),
                ];

                let weight_str;
                if let Some(ref weight) = rel.weight {
                    weight_str = weight.to_string();
                    edge_props.push((PROP_WEIGHT, &weight_str));
                }

                let props_str = Self::gql_props(&edge_props);
                let gql = format!(
                    "MATCH (a), (b) WHERE a.uuid = '{}' AND b.uuid = '{}' INSERT (a)-[e:{} {}]->(b)",
                    Self::gql_escape(&rel.from_id),
                    Self::gql_escape(&rel.to_id),
                    edge_type_stored,
                    props_str,
                );
                txn.execute_gql_write(&gql)?;

                Ok(StreamOpResult::RelationshipCreated(GraphEdge {
                    id: edge_uuid,
                    edge_type: rel.edge_type.clone(),
                    from_id: rel.from_id.clone(),
                    to_id: rel.to_id.clone(),
                    properties: HashMap::new(),
                    created_at: now,
                    weight: rel.weight,
                }))
            }
            StreamOp::DeleteRelationship(id) => {

                let gql = format!(
                    "MATCH ()-[e]->() WHERE e.uuid = '{}' DELETE e",
                    Self::gql_escape(id),
                );
                txn.execute_gql_write(&gql)?;
                Ok(StreamOpResult::RelationshipDeleted)
            }
            StreamOp::SetConfig { key, value } => {
                let value_str = serde_json::to_string(value)?;
                let delete_gql = format!(
                    "MATCH (n:{}) WHERE n.{} = '{}' DELETE n",
                    LABEL_CONFIG,
                    PROP_UUID,
                    Self::gql_escape(key),
                );
                let _ = txn.execute_gql_write(&delete_gql);

                let props = Self::gql_props(&[
                    (PROP_UUID, key),
                    (PROP_CONFIG_VALUE, &value_str),
                ]);
                let create_gql = format!("INSERT (n:{} {})", LABEL_CONFIG, props);
                txn.execute_gql_write(&create_gql)?;
                Ok(StreamOpResult::ConfigSet)
            }
            StreamOp::RawGql(stmt) => {
                let output = txn.execute_gql(stmt)?;
                // Try to extract a result value from the GQL response
                let json_result = match output {
                    StatementOutput::Rows(table) => {
                        // Convert first row first column to JSON if possible
                        if let Some(row) = table.rows().first() {
                            if let Some(idx) = table.column_index(crate::graph::to_db_string("n")) {
                                if let Some(val) = row.get(idx) {
                                    selene_value_to_json(val)
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                Ok(StreamOpResult::RawGql(json_result))
            }
            StreamOp::MergeNode(input) => {
                // Merge (upsert) a node by (node_type, name) uniqueness constraint
                let graph_node = Self::create_node_from_input(input.clone());
                // Check if a node with this type+name already exists
                let existing = Self::find_node_by_type_and_name(txn, &graph_node.node_type, &graph_node.name)?;
                if let Some(existing_node) = existing {
                    // Update existing node
                    let now = Utc::now();
                    let mut updated = existing_node.clone();
                    updated.properties = graph_node.properties;
                    updated.description = graph_node.description;
                    updated.subtype = graph_node.subtype;
                    updated.updated_at = now;
                    updated.version = existing_node.version + 1;

                    let delete_gql = format!(
                        "MATCH (n) WHERE n.uuid = '{}' DETACH DELETE n",
                        Self::gql_escape(&existing_node.id),
                    );
                    txn.execute_gql_write(&delete_gql)?;

                    let node_type_json = serde_json::to_string(&updated.node_type)
                        .unwrap_or_else(|_| format!("{:?}", updated.node_type));
                    let node_type_stored = node_type_json.trim_matches('"').to_string();
                    let description_val = updated.description.as_deref().unwrap_or("");
                    let subtype_val = updated.subtype.as_deref().unwrap_or("");
                    let embedding_id_val = updated.embedding_id.as_deref().unwrap_or("");
                    let created_at_str = updated.created_at.to_rfc3339();
                    let updated_at_str = updated.updated_at.to_rfc3339();
                    let version_str = updated.version.to_string();

                    // Serialize custom properties as native GQL types
                    let mut extra_parts: Vec<String> = Vec::new();
                    for (key, val) in &updated.properties {
                        extra_parts.push(format!("{}: {}", key, Self::format_value_as_gql(val)));
                    }

                    let props = [
                        (PROP_UUID, updated.id.as_str()),
                        (PROP_NODE_TYPE, node_type_stored.as_str()),
                        (PROP_NAME, updated.name.as_str()),
                        (PROP_DESCRIPTION, description_val),
                        (PROP_SUBTYPE, subtype_val),
                        (PROP_EMBEDDING_ID, embedding_id_val),
                        (PROP_CREATED_AT, created_at_str.as_str()),
                        (PROP_UPDATED_AT, updated_at_str.as_str()),
                        (PROP_VERSION, version_str.as_str()),
                    ];
                    let mut parts: Vec<String> = props.iter()
                        .map(|(k, v)| format!("{}: '{}'", k, Self::gql_escape(v)))
                        .collect();
                    parts.extend(extra_parts);
                    let props_str = format!("{{{}}}", parts.join(", "));
                    let gql = format!("INSERT (n:{} {})", LABEL_SPIRE_NODE, props_str);
                    txn.execute_gql_write(&gql)?;
                    Ok(StreamOpResult::NodeUpdated(updated))
                } else {
                    // Insert new node
                    Self::store_node_in_txn(txn, &graph_node, None)?;
                    Ok(StreamOpResult::NodeStored(graph_node))
                }
            }
            StreamOp::MergeRelationship(rel) => {
                // Merge (upsert) a relationship by (edge_type, from_id, to_id) uniqueness constraint
                let edge_uuid = Uuid::new_v4().to_string();
                let now = Utc::now();
                let edge_type_stored = relationship_type_to_gql_label(&rel.edge_type);

                // Delete existing relationship with same edge_type, from_id, to_id

                let delete_gql = format!(
                    "MATCH (a)-[e:{}]->(b) WHERE a.uuid = '{}' AND b.uuid = '{}' DELETE e",
                    edge_type_stored,
                    Self::gql_escape(&rel.from_id),
                    Self::gql_escape(&rel.to_id),
                );
                let _ = txn.execute_gql_write(&delete_gql);

                // Create new relationship
                let created_at_str = now.to_rfc3339();
                let mut edge_props: Vec<(&str, &str)> = vec![
                    (PROP_UUID, &edge_uuid),
                    (PROP_EDGE_TYPE, &edge_type_stored),
                    (PROP_CREATED_AT, &created_at_str),
                ];

                let weight_str;
                if let Some(ref weight) = rel.weight {
                    weight_str = weight.to_string();
                    edge_props.push((PROP_WEIGHT, &weight_str));
                }

                let props_str = Self::gql_props(&edge_props);
                let gql = format!(
                    "MATCH (a), (b) WHERE a.uuid = '{}' AND b.uuid = '{}' INSERT (a)-[e:{} {}]->(b)",
                    Self::gql_escape(&rel.from_id),
                    Self::gql_escape(&rel.to_id),
                    edge_type_stored,
                    props_str,
                );
                txn.execute_gql_write(&gql)?;

                Ok(StreamOpResult::RelationshipCreated(GraphEdge {
                    id: edge_uuid,
                    edge_type: rel.edge_type.clone(),
                    from_id: rel.from_id.clone(),
                    to_id: rel.to_id.clone(),
                    properties: HashMap::new(),
                    created_at: now,
                    weight: rel.weight,
                }))
            }
            StreamOp::Commit | StreamOp::Rollback => {
                // These are handled by the stream loop, not here
                Ok(StreamOpResult::RawGql(None))
            }
        }
    }
}

// ============================================================================
// Actor trait implementation
// ============================================================================

#[async_trait]
impl Actor for MemoryGraphActor {

    type Message = MemoryGraphMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            // ── Lifecycle ────────────────────────────────────────────
            MemoryGraphMessage::Initialize { data_dir, reply_to } => {
                let result = self.init_graph(&data_dir);
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::InitializeEmbedder { model_path, embedder, reply_to } => {
                let result = self.init_embedder(model_path, embedder);
                let _ = reply_to.send(result);
            }

            // ── Node Operations ──────────────────────────────────────
            MemoryGraphMessage::GetNode { id, reply_to } => {
                let result = match self.query_node_by_uuid(&id) {
                    Some(node) => Ok(Some(node)),
                    None => Ok(None),
                };
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::QueryNodes { filter, reply_to } => {
                let nodes = self.query_nodes(filter);
                let _ = reply_to.send(Ok(nodes));
            }
            MemoryGraphMessage::StoreNode { node, reply_to } => {
                let result = (|| -> Result<GraphNode> {
                    let graph_node = Self::create_node_from_input(node);

                    // Check for duplicate (type, name)
                    if self.has_duplicate(&graph_node.node_type, &graph_node.name) {
                        return Err(anyhow::anyhow!(
                            "Node with type '{:?}' and name '{}' already exists",
                            graph_node.node_type,
                            graph_node.name,
                        ));
                    }

                    self.store_node_via_gql(&graph_node, None)?;
                    self.schedule_snapshot();
                    Ok(graph_node)
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::UpdateNode { id, updates, reply_to } => {
                let result = (|| -> Result<GraphNode> {
                    let existing = self.query_node_by_uuid(&id)
                        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", id))?;
                    let updated = Self::apply_updates(&existing, updates);

                    // Delete old node and create new one
                    self.delete_node_via_gql(&id)?;
                    self.store_node_via_gql(&updated, None)?;
                    self.schedule_snapshot();
                    Ok(updated)
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::DeleteNode { id, reply_to } => {
                let result = self.delete_node_via_gql(&id);
                if result.is_ok() {
                    self.schedule_snapshot();
                }
                let _ = reply_to.send(result);
            }

            // ── Relationship Operations ──────────────────────────────
            MemoryGraphMessage::CreateRelationship { rel, reply_to } => {
                let result = (|| -> Result<GraphEdge> {
                    // Verify both nodes exist
                    self.query_node_by_uuid(&rel.from_id)
                        .ok_or_else(|| anyhow::anyhow!("Source node not found: {}", rel.from_id))?;
                    self.query_node_by_uuid(&rel.to_id)
                        .ok_or_else(|| anyhow::anyhow!("Target node not found: {}", rel.to_id))?;

                    // Check for cycles if this is a DependsOn relationship
                    if rel.edge_type == RelationshipType::DependsOn {
                        if self.would_create_cycle(&rel.from_id, &rel.to_id) {
                            return Err(anyhow::anyhow!(
                                "Adding this DependsOn edge would create a cycle"
                            ));
                        }
                    }

                    let edge_uuid = Uuid::new_v4().to_string();
                    let now = Utc::now();
                    let edge_type_stored = relationship_type_to_gql_label(&rel.edge_type);

                    let created_at_str = now.to_rfc3339();
                    let mut edge_props: Vec<(&str, &str)> = vec![
                        (PROP_UUID, &edge_uuid),
                        (PROP_EDGE_TYPE, &edge_type_stored),
                        (PROP_CREATED_AT, &created_at_str),
                    ];

                    let weight_str;
                    if let Some(ref weight) = rel.weight {
                        weight_str = weight.to_string();
                        edge_props.push((PROP_WEIGHT, &weight_str));
                    }

                    self.store_edge_via_gql(&rel.from_id, &edge_type_stored, &rel.to_id, &edge_props)?;

                    self.schedule_snapshot();

                    Ok(GraphEdge {
                        id: edge_uuid,
                        edge_type: rel.edge_type,
                        from_id: rel.from_id,
                        to_id: rel.to_id,
                        properties: HashMap::new(),
                        created_at: now,
                        weight: rel.weight,
                    })
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::GetRelationships { node_id, reply_to } => {
                let edges = self.query_edges_for_node(&node_id);
                let _ = reply_to.send(Ok(edges));
            }
            MemoryGraphMessage::DeleteRelationship { id, reply_to } => {
                let result = self.delete_edge_via_gql(&id);
                if result.is_ok() {
                    self.schedule_snapshot();
                }
                let _ = reply_to.send(result);
            }

            // ── Traversal ────────────────────────────────────────────
            MemoryGraphMessage::Traverse { start_node_id, options, reply_to } => {
                let result = self.traverse(&start_node_id, &options);
                let _ = reply_to.send(result);
            }

            // ── Context & Memory ─────────────────────────────────────
            MemoryGraphMessage::GetProjectContext { reply_to } => {
                let result = (|| -> Result<ProjectSnapshot> {
                    let nodes = self.query_all_spire_nodes();
                    let stats = ProjectStats {
                        total_nodes: nodes.len(),
                        total_relationships: self.graph_db.as_ref().map(|g| g.edge_count()).unwrap_or(0),
                        last_updated: Utc::now(),
                    };
                    // Find the Project node — the first Project-type node found, or a placeholder.
                    let project = nodes.iter().find(|n| n.node_type == NodeType::Project)
                        .cloned()
                        .unwrap_or_else(|| GraphNode {
                            id: "unknown".to_string(),
                            node_type: NodeType::Project,
                            subtype: None,
                            name: "Unknown Project".to_string(),
                            description: None,
                            properties: HashMap::new(),
                            embedding_id: None,
                            created_at: Utc::now(),
                            updated_at: Utc::now(),
                            version: 1,
                        });

                    // Query context nodes by type from the graph.
                    // active_context: the most recent ActiveContext node, if any.
                    let mut active_context_nodes: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::ActiveContext)
                        .cloned()
                        .collect();
                    active_context_nodes.sort_by(|a, b| b.created_at.cmp(&a.created_at));
                    let active_context = active_context_nodes.into_iter().next();

                    // milestones: all Milestone nodes, sorted by recency.
                    let mut milestones: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::Milestone)
                        .cloned()
                        .collect();
                    milestones.sort_by(|a, b| b.created_at.cmp(&a.created_at));

                    // blockers: all Blocker nodes, sorted by recency.
                    let mut blockers: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::Blocker)
                        .cloned()
                        .collect();
                    blockers.sort_by(|a, b| b.created_at.cmp(&a.created_at));

                    // recent_decisions: all Decision nodes, sorted by recency.
                    let mut recent_decisions: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::Decision)
                        .cloned()
                        .collect();
                    recent_decisions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

                    // recent_entities: all Entity nodes, sorted by recency.
                    let mut recent_entities: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::Entity)
                        .cloned()
                        .collect();
                    recent_entities.sort_by(|a, b| b.created_at.cmp(&a.created_at));

                    // standards: all Standard nodes, sorted by recency.
                    let mut standards: Vec<GraphNode> = nodes.iter()
                        .filter(|n| n.node_type == NodeType::Standard)
                        .cloned()
                        .collect();
                    standards.sort_by(|a, b| b.created_at.cmp(&a.created_at));

                    Ok(ProjectSnapshot {
                        project,
                        active_context,
                        milestones,
                        blockers,
                        recent_decisions,
                        recent_entities,
                        standards,
                        stats,
                    })
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::SearchContext { query, options, reply_to } => {
                let result = self.handle_search_context(query, options).await;
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::AddMemory { text, metadata, reply_to } => {
                let result = self.handle_add_memory(text, metadata).await;
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::Recall { query, limit, reply_to } => {
                let result = self.handle_recall(query, limit).await;
                let _ = reply_to.send(result);
            }

            // ── Config Storage ───────────────────────────────────────
            MemoryGraphMessage::SetConfig { key, value, reply_to } => {
                let result = (|| -> Result<()> {
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    let value_str = serde_json::to_string(&value)?;

                    // Upsert: delete existing then create
                    let delete_gql = format!(
                        "MATCH (n:{}) WHERE n.{} = '{}' DELETE n",
                        LABEL_CONFIG,
                        PROP_UUID,
                        Self::gql_escape(&key),
                    );
                    let _ = graph_db.execute_gql_write(&delete_gql);

                    let props = Self::gql_props(&[
                        (PROP_UUID, &key),
                        (PROP_CONFIG_VALUE, &value_str),
                    ]);
                    let create_gql = format!("INSERT (n:{} {})", LABEL_CONFIG, props);
                    graph_db.execute_gql_write(&create_gql)?;
                    self.schedule_snapshot();
                    Ok(())
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::GetConfig { key, reply_to } => {
                let result = (|| -> Result<Option<serde_json::Value>> {
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    let gql = format!(
                        "MATCH (n:{}) WHERE n.{} = '{}' RETURN n.{}",
                        LABEL_CONFIG,
                        PROP_UUID,
                        Self::gql_escape(&key),
                        PROP_CONFIG_VALUE,
                    );
                    match graph_db.execute_gql_query(&gql) {
                        Ok(Some(table)) => {
                            if let Some(row) = table.rows().first() {
                                // SeleneDB property projections produce columns with name: None,
                                // so we must use positional index 0.
                                if let Some(Value::String(val)) = row.get(0) {
                                    let json: serde_json::Value = serde_json::from_str(&val.to_string())?;
                                    return Ok(Some(json));
                                }
                            }
                            Ok(None)
                        }
                        _ => Ok(None),
                    }

                })();
                let _ = reply_to.send(result);
            }

            // ── MCP Config Storage ───────────────────────────────────
            MemoryGraphMessage::BootstrapMcpConfig { config_path, reply_to } => {
                let result = (|| -> Result<()> {
                    let content = std::fs::read_to_string(&config_path)?;
                    let config: McpConfigFile = serde_json::from_str(&content)?;
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    // Sync semantics: delete all existing SpireMcpConfig nodes first,
                    // then insert all servers from the config file.
                    let _ = graph_db.execute_gql_write("MATCH (n:SpireMcpConfig) DELETE n");

                    for server in config.servers {
                        let entry = McpServerConfigEntry {
                            name: server.name,
                            command: server.command,
                            args: server.args,
                            env: server.env,
                            url: server.url,
                            headers: server.headers,
                            autostart: server.autostart,
                        };
                        let entry_json = serde_json::to_string(&entry)?;
                        let props = Self::gql_props(&[
                            (PROP_UUID, &entry.name),
                            (PROP_CONFIG_VALUE, &entry_json),
                        ]);
                        let gql = format!("INSERT (n:SpireMcpConfig {})", props);
                        graph_db.execute_gql_write(&gql)?;
                    }
                    self.schedule_snapshot();
                    Ok(())
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::GetMcpConfig { reply_to } => {
                let result = (|| -> Result<Vec<McpServerConfigEntry>> {
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    // NOTE: We use RETURN n.config_value instead of RETURN n because
                    // SeleneDB returns a NodeRef (node ID reference) when projecting
                    // the whole node, not a Record with properties. By projecting the
                    // specific property, we get the value directly as a String.
                    let gql = format!("MATCH (n:SpireMcpConfig) RETURN n.{}", PROP_CONFIG_VALUE);
                    tracing::info!("GetMcpConfig: executing GQL: {}", gql);
                    let mut entries = Vec::new();
                    match graph_db.execute_gql_query(&gql) {
                        Ok(Some(table)) => {
                            tracing::info!("GetMcpConfig: got table with {} rows",
                                table.rows().len());
                            // SeleneDB property projections (RETURN n.config_value) produce columns
                            // with name: None in the schema, so we must use positional index 0.
                            for (row_idx, row) in table.rows().iter().enumerate() {
                                tracing::info!("GetMcpConfig: processing row {}", row_idx);
                                if let Some(Value::String(json_str)) = row.get(0) {
                                    tracing::info!("GetMcpConfig: found config_value: {}", json_str.to_string());
                                    if let Ok(entry) = serde_json::from_str::<McpServerConfigEntry>(&json_str.to_string()) {
                                        entries.push(entry);
                                    } else {
                                        tracing::warn!("GetMcpConfig: failed to parse McpServerConfigEntry from: {}", json_str.to_string());
                                    }
                                } else {
                                    tracing::warn!("GetMcpConfig: column 0 is not a String in row {} (got {:?})",
                                        row_idx, row.get(0));
                                }
                            }
                        }

                        Ok(None) => {
                            tracing::info!("GetMcpConfig: query returned None (no rows)");
                        }
                        Err(e) => {
                            tracing::warn!("GetMcpConfig: query error: {}", e);
                        }
                    }

                    tracing::info!("GetMcpConfig: returning {} entries", entries.len());
                    Ok(entries)
                })();
                let _ = reply_to.send(result);
            }
            // ── Batch / Atomic Operations ────────────────────────────
            MemoryGraphMessage::SeedIntents { config_path, reply_to } => {
                let result = async {
                    let content = std::fs::read_to_string(&config_path)
                        .map_err(|e| anyhow::anyhow!("Failed to read intents config: {}", e))?;
                    let intents_config: serde_json::Value = serde_json::from_str(&content)
                        .map_err(|e| anyhow::anyhow!("Failed to parse intents config: {}", e))?;
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    // Extract intents array
                    let intents = intents_config.get("intents")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| anyhow::anyhow!("Missing or invalid 'intents' array in config"))?;

                    // Track created nodes for post-insert embedding generation
                    let mut created_nodes: Vec<(String, String, String)> = Vec::new(); // (uuid, name, text_for_embedding)

                    for intent in intents {
                        let name = intent.get("name")
                            .and_then(|v| v.as_str())
                            .ok_or_else(|| anyhow::anyhow!("Intent missing 'name' field"))?;
                        let description = intent.get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("");

                        // Build a rich text for embedding: description + patterns
                        let patterns = intent.get("patterns")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter()
                                .filter_map(|p| p.as_str())
                                .collect::<Vec<_>>()
                                .join(" "))
                            .unwrap_or_default();
                        let embed_text = if patterns.is_empty() {
                            format!("{}: {}", name, description)
                        } else {
                            format!("{}: {}. Patterns: {}", name, description, patterns)
                        };

                        // Use a deterministic UUID based on the intent name
                        let uuid = format!("intent-{}", name);

                        // Store as a SpireNode with node_type=Standard + subtype=intent
                        // Build property map: standard fields + all intent config fields
                        let mut prop_parts: Vec<String> = vec![
                            format!("{}: '{}'", PROP_UUID, Self::gql_escape(&uuid)),
                            format!("{}: '{}'", PROP_NODE_TYPE, "Standard"),
                            format!("{}: '{}'", PROP_SUBTYPE, "intent"),
                            format!("{}: '{}'", PROP_NAME, Self::gql_escape(name)),
                            format!("{}: '{}'", PROP_DESCRIPTION, Self::gql_escape(description)),
                        ];

                        // Store patterns as a GQL array of strings
                        if let Some(patterns) = intent.get("patterns").and_then(|v| v.as_array()) {
                            let pattern_strs: Vec<String> = patterns.iter()
                                .filter_map(|p| p.as_str())
                                .map(|p| format!("'{}'", Self::gql_escape(p)))
                                .collect();
                            prop_parts.push(format!("patterns: [{}]", pattern_strs.join(", ")));
                        }

                        // Store priority as a GQL integer
                        if let Some(priority) = intent.get("priority").and_then(|v| v.as_u64()) {
                            prop_parts.push(format!("priority: {}", priority));
                        }

                        // Store handler as a GQL string
                        if let Some(handler) = intent.get("handler").and_then(|v| v.as_str()) {
                            prop_parts.push(format!("handler: '{}'", Self::gql_escape(handler)));
                        }

                        // Store action as a GQL string
                        if let Some(action) = intent.get("action").and_then(|v| v.as_str()) {
                            prop_parts.push(format!("action: '{}'", Self::gql_escape(action)));
                        }

                        // Store requires_approval as a GQL boolean
                        if let Some(requires_approval) = intent.get("requires_approval").and_then(|v| v.as_bool()) {
                            prop_parts.push(format!("requires_approval: {}", requires_approval));
                        }

                        // Store state_requirements as a GQL array of strings
                        if let Some(state_reqs) = intent.get("state_requirements").and_then(|v| v.as_array()) {
                            let req_strs: Vec<String> = state_reqs.iter()
                                .filter_map(|r| r.as_str())
                                .map(|r| format!("'{}'", Self::gql_escape(r)))
                                .collect();
                            prop_parts.push(format!("state_requirements: [{}]", req_strs.join(", ")));
                        }

                        let props_str = format!("{{{}}}", prop_parts.join(", "));
                        let gql = format!("INSERT (n:SpireNode {})", props_str);
                        graph_db.execute_gql_write(&gql)?;

                        created_nodes.push((uuid, name.to_string(), embed_text));
                    }

                    // Generate embeddings for each intent node (deferred, after insert)
                    if let Some(embedder) = self.embedder.as_ref() {
                        for (uuid, name, embed_text) in &created_nodes {
                            match embedder.embed(embed_text).await {
                                Ok(embedding) => {
                                    let vec_str: Vec<String> = embedding.vector.iter()
                                        .map(|v| v.to_string())
                                        .collect();
                                    let vec_list = format!("[{}]", vec_str.join(", "));
                                    let set_gql = format!(
                                        "MATCH (n) WHERE n.{} = '{}' SET n.embedding = {}",
                                        PROP_UUID,
                                        Self::gql_escape(uuid),
                                        vec_list,
                                    );
                                    if let Err(e) = graph_db.execute_gql_write(&set_gql) {
                                        warn!("SeedIntents: failed to set embedding for intent '{}': {}", name, e);
                                    }
                                }
                                Err(e) => {
                                    warn!("SeedIntents: failed to embed intent '{}': {}", name, e);
                                }
                            }
                        }
                    } else {
                        info!("SeedIntents: no embedder available, skipping intent embeddings");
                    }

                    self.schedule_snapshot();
                    Ok::<_, anyhow::Error>(())
                }.await;
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::AtomicUpdateNode { id, updates, reply_to } => {
                let result = (|| -> Result<GraphNode> {
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    let existing = self.query_node_by_uuid(&id)
                        .ok_or_else(|| anyhow::anyhow!("Node not found: {}", id))?;
                    let updated = Self::apply_updates(&existing, updates);

                    // Use an explicit transaction for atomic delete + insert
                    let mut txn = graph_db.begin_transaction()?;
                    txn.execute_gql_write(&format!(
                        "MATCH (n) WHERE n.uuid = '{}' DETACH DELETE n",
                        Self::gql_escape(&id),
                    ))?;
                    self.store_node_via_gql(&updated, None)?;
                    txn.commit()?;

                    self.schedule_snapshot();
                    Ok(updated)
                })();
                let _ = reply_to.send(result);
            }
            MemoryGraphMessage::BatchGql { statements, reply_to } => {
                let result = (|| -> Result<()> {
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    let mut txn = graph_db.begin_transaction()?;
                    for stmt in &statements {
                        txn.execute_gql(stmt)?;
                    }
                    txn.commit()?;
                    self.schedule_snapshot();
                    Ok(())
                })();
                let _ = reply_to.send(result);
            }

            // ── Transaction Stream ────────────────────────────────────
            MemoryGraphMessage::OpenTransactionStream { reply_to } => {
                let graph_db = match self.graph_db.as_ref() {
                    Some(db) => db.clone(),
                    None => {
                        let _ = reply_to.send(mpsc::channel(64).0); // dummy sender that will be dropped
                        return;
                    }
                };

                let (tx, mut rx): (mpsc::Sender<TransactionRequest>, mpsc::Receiver<TransactionRequest>) = mpsc::channel(64);

                // Clone the Arc so the spawned thread can hold a reference
                let snapshot_tx = self.snapshot_tx.clone();

                // GraphDbTransaction is !Send, so we must create AND use the transaction
                // on a dedicated OS thread rather than a tokio task.
                std::thread::spawn(move || {
                    // Convert the async mpsc receiver into a blocking receiver
                    // Open the underlying SeleneDB transaction ON THIS THREAD.
                    // GraphDbTransaction is !Send, so it MUST be created on the same
                    // thread that uses it. Previously this was done on the actor thread
                    // and moved into the closure, which caused writes to silently fail
                    // because SeleneDB's WriteTxn uses thread-local state.
                    // We wrap it in Option so we can take() ownership when committing/rolling back,
                    // satisfying the borrow checker that txn is not used after being consumed.
                    let mut txn: Option<crate::graph::GraphDbTransaction> = match graph_db.begin_transaction() {
                        Ok(t) => Some(t),
                        Err(e) => {
                            warn!("Transaction stream: failed to begin transaction: {}", e);
                            return;
                        }
                    };


                    // Blocking receive loop — the mpsc channel is Send, so we can
                    // use a simple polling approach with a short timeout.
                    let outcome: Option<Result<(), String>> = loop {
                        // Try to receive with a short timeout to allow checking
                        // for shutdown without blocking indefinitely.
                        let request = match rx.blocking_recv() {
                            Some(req) => req,
                            None => {
                                // Channel closed (sender dropped)
                                break None;
                            }
                        };

                        let is_commit = matches!(request.operation, StreamOp::Commit);
                        let is_rollback = matches!(request.operation, StreamOp::Rollback);

                        // Execute the operation. We scope the mutable borrow of txn
                        // so it's dropped before we take() ownership below.
                        let op_result = {
                            let txn_ref = txn.as_mut().unwrap();
                            Self::execute_stream_op_in_txn(txn_ref, &request.operation)
                        };

                        if is_commit {
                            match op_result {
                                Ok(_) => {
                                    let commit_result = txn.take().unwrap().commit();
                                    match commit_result {
                                        Ok(_) => {
                                let _ = request.reply_to.send(Ok(StreamOpResult::RawGql(None)));
                                if let Some(ref stx) = snapshot_tx {
                                    let _ = stx.send(());
                                }
                                break Some(Ok(()));
                                        }
                                        Err(e) => {
                                            let _ = request.reply_to.send(Err(format!("Commit failed: {}", e)));
                                            break Some(Err(format!("Commit failed: {}", e)));
                                        }
                                    }
                                }
                                Err(e) => {
                                    let _ = request.reply_to.send(Err(format!("Commit pre-check failed: {}", e)));
                                    break Some(Err(format!("Commit pre-check failed: {}", e)));
                                }
                            }
                        } else if is_rollback {
                            let _ = txn.take().unwrap().rollback();
                            let _ = request.reply_to.send(Ok(StreamOpResult::RawGql(None)));
                            break Some(Ok(()));
                        } else {
                            let is_err = op_result.is_err();
                            let _ = request.reply_to.send(op_result.map_err(|e| e.to_string()));
                            if is_err {
                                // On error, roll back the transaction
                                warn!("Transaction stream: operation failed, rolling back");
                                let _ = txn.take().unwrap().rollback();
                                break Some(Err("Operation failed, rolled back".to_string()));
                            }
                        }
                    };

                    // If the sender was dropped without explicit Commit/Rollback, commit automatically
                    if outcome.is_none() {
                        if let Some(t) = txn.take() {
                            info!("Transaction stream: sender dropped, committing automatically");
                            t.commit().ok();
                            if let Some(ref stx) = snapshot_tx {
                                let _ = stx.send(());
                            }
                        }
                    }
                });

                let _ = reply_to.send(tx);
            }

            // ── Maintenance ──────────────────────────────────────────
            MemoryGraphMessage::Sync { reply_to } => {

                let result = (|| -> Result<()> {
                    let data_dir = self.data_dir.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("data_dir not set"))?;
                    let graph_db = self.graph_db.as_ref()
                        .ok_or_else(|| anyhow::anyhow!("GraphDb not initialized"))?;

                    let next_seq = match GraphDb::latest_snapshot_sequence(data_dir)? {
                        Some(seq) => seq + 1,
                        None => 1,
                    };

                    let outcome = graph_db.write_snapshot(data_dir, next_seq, true)?;
                    info!(
                        "MemoryGraph: snapshot written (seq={}, sections={})",
                        outcome.snapshot_seq,
                        outcome.section_count,
                    );
                    graph_db.compact()?;
                    Ok(())
                })();
                let _ = reply_to.send(result);
            }
        }
    }
}

// ============================================================================
// Value Conversion Helpers
// ============================================================================

/// Convert a `serde_json::Value` to a `selene_db_core::value::Value`.
fn json_value_to_selene(json: &serde_json::Value) -> Option<Value> {
    match json {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(b) => Some(Value::Bool(*b)),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Some(Value::Int(i))
            } else if let Some(f) = n.as_f64() {
                Some(Value::Float(f))
            } else {
                None
            }
        }
        serde_json::Value::String(s) => Some(Value::String(
            selene_db_core::db_string::DbString::try_from(s.clone()).unwrap()
        )),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().filter_map(json_value_to_selene).collect();
            Some(Value::List(items))
        }
        serde_json::Value::Object(obj) => {
            use selene_db_core::value::Record;
            let mut fields = Vec::new();
            for (k, v) in obj {
                if let Some(val) = json_value_to_selene(v) {
                    fields.push((
                        selene_db_core::db_string::DbString::try_from(k.clone()).unwrap(),
                        val,
                    ));
                }
            }
            Some(Value::Record(Box::new(Record::Open(fields.into()))))
        }
    }
}

/// Convert a `selene_db_core::value::Value` to a `serde_json::Value`.
fn selene_value_to_json(val: &Value) -> Option<serde_json::Value> {
    match val {
        Value::Null => Some(serde_json::Value::Null),
        Value::Bool(b) => Some(serde_json::Value::Bool(*b)),
        Value::Int(i) => Some(serde_json::Value::Number(serde_json::Number::from(*i))),
        Value::Float(f) => {
            serde_json::Number::from_f64(*f).map(serde_json::Value::Number)
        }
        Value::String(s) => Some(serde_json::Value::String(s.to_string())),
        Value::List(list) => {
            let items: Vec<serde_json::Value> = list.iter().filter_map(selene_value_to_json).collect();
            Some(serde_json::Value::Array(items))
        }
        Value::Record(record) => {
            let mut obj = serde_json::Map::new();
            match record.as_ref() {
                selene_db_core::value::Record::Open(fields) => {
                    for (k, v) in fields.iter() {
                        if let Some(json_val) = selene_value_to_json(v) {
                            obj.insert(k.to_string(), json_val);
                        }
                    }
                }
                _ => {}
            }
            Some(serde_json::Value::Object(obj))
        }
        _ => None,
    }
}
