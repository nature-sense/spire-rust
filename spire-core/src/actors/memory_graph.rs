// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! MemoryGraphActor — backed by SeleneDB's `GraphDb` with GQL persistence.
//!
//! This actor is the sole data store for the system, owning graph nodes, edges,
//! and vector embeddings. All storage is delegated to `GraphDb` (SeleneDB),
//! which provides lock-free reads, serialized writes, and optional WAL persistence.
//!
//! Unlike the previous implementation which stored all metadata in HashMaps,
//! this version persists everything directly in SeleneDB using GQL statements
//! via `selene_gql::runtime::Session`. This means all data survives restarts
//! when WAL persistence is enabled.
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
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use selene_db_core::identity::{EdgeId, NodeId};
use selene_db_core::value::Value;
use selene_db_core::vector::VectorMetric;

use crate::actors::Actor;
use crate::graph::GraphDb;
use crate::models::embedding::Embedder;
use crate::models::graph::GraphResult;
use crate::models::memory_graph::{
    ContextSearchResult, GraphEdge, GraphNode, MemoryEntry, MemoryMetadata, NodeFilter, NodeInput,
    NodeType, NodeUpdate, ProjectSnapshot, ProjectStats, RelationshipInput, RelationshipType,
    RetrievalSource, SchemaError, ScoredNode, SearchOptions, TraversalDirection, TraversalOptions,
    TraversalResult,
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

// ============================================================================
// MemoryGraphMessage Enum — 14 variants matching IMemoryGraph API
// ============================================================================

/// Messages for the MemoryGraph actor.
///
/// This actor is the sole data store for the system, owning graph nodes, edges,
/// and vector embeddings directly (no separate GraphActor or VectorActor).
pub enum MemoryGraphMessage {
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
    graph_db: Arc<GraphDb>,

    /// UUID → SeleneDB NodeId mapping (cached for performance).
    /// The external API uses UUID strings; SeleneDB uses compact u64 IDs.
    /// This cache is rebuilt on startup by scanning the graph.
    uuid_to_node: HashMap<String, NodeId>,

    /// UUID → SeleneDB EdgeId mapping (cached for performance).
    uuid_to_edge: HashMap<String, EdgeId>,

    /// Reverse mapping: SeleneDB NodeId → UUID.
    node_to_uuid: HashMap<NodeId, String>,

    /// Reverse mapping: SeleneDB EdgeId → UUID.
    edge_to_uuid: HashMap<EdgeId, String>,

    /// Embedder for text → vector generation.
    embedder: Arc<dyn Embedder>,
}

impl MemoryGraphActor {
    pub fn new(graph_db: Arc<GraphDb>, embedder: Arc<dyn Embedder>) -> Self {
        let mut actor = Self {
            graph_db,
            uuid_to_node: HashMap::new(),
            uuid_to_edge: HashMap::new(),
            node_to_uuid: HashMap::new(),
            edge_to_uuid: HashMap::new(),
            embedder,
        };

        // Rebuild the UUID ↔ SeleneDB ID cache from the persisted graph.
        if let Err(e) = actor.rebuild_cache() {
            info!("MemoryGraph: cache rebuild skipped (empty graph): {}", e);
        }

        actor
    }

    /// Rebuild the UUID ↔ SeleneDB ID cache by scanning all nodes and edges.
    fn rebuild_cache(&mut self) -> Result<()> {
        let snapshot = self.graph_db.shared_graph().read();

        // Scan all SpireNode-labeled nodes
        if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_SPIRE_NODE).unwrap()) {
            for row in bitmap.iter() {
                if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                    if let Some(props) = snapshot.node_properties(node_id) {
                        if let Some(uuid_val) = props.get(&selene_db_core::db_string::DbString::try_from(PROP_UUID).unwrap()) {
                            if let Value::String(uuid_str) = uuid_val {
                                let uuid = uuid_str.to_string();
                                self.uuid_to_node.insert(uuid.clone(), node_id);
                                self.node_to_uuid.insert(node_id, uuid);
                            }
                        }
                    }
                }
            }
        }

        // Scan all edges for UUID properties
        let live_edges = snapshot.live_edges().clone();
        for row in live_edges.iter() {
            if let Some(edge_id) = snapshot.edge_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                if let Some(props) = snapshot.edge_properties(edge_id) {
                    if let Some(uuid_val) = props.get(&selene_db_core::db_string::DbString::try_from(PROP_UUID).unwrap()) {
                            if let Value::String(uuid_str) = uuid_val {
                                let uuid = uuid_str.to_string();
                                self.uuid_to_edge.insert(uuid.clone(), edge_id);
                                self.edge_to_uuid.insert(edge_id, uuid);
                        }
                    }
                }
            }
        }

        info!(
            "MemoryGraph: cache rebuilt — {} nodes, {} edges",
            self.uuid_to_node.len(),
            self.uuid_to_edge.len()
        );

        Ok(())
    }

    /// Check whether a node with the given `(type, name)` already exists.
    fn has_duplicate(&self, node_type: &NodeType, name: &str) -> bool {
        let snapshot = self.graph_db.shared_graph().read();
        let type_str = format!("{:?}", node_type);

        if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_SPIRE_NODE).unwrap()) {
            for row in bitmap.iter() {
                if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                    if let Some(props) = snapshot.node_properties(node_id) {
                        let type_match = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_NODE_TYPE).unwrap())
                            .map_or(false, |v| matches!(v, Value::String(s) if s.as_str() == type_str));
                        let name_match = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_NAME).unwrap())
                            .map_or(false, |v| matches!(v, Value::String(s) if s.as_str() == name));
                        if type_match && name_match {
                            return true;
                        }
                    }
                }
            }
        }
        false
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

    /// Build a `GraphNode` from SeleneDB node properties.
    fn build_node_from_props(_node_id: NodeId, props: &selene_db_core::property_map::PropertyMap) -> Option<GraphNode> {
        let uuid = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_UUID).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.to_string())
                } else {
                    None
                }
            })?;

        let node_type_str = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_NODE_TYPE).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.to_string())
                } else {
                    None
                }
            })?;

        let node_type = serde_json::from_str::<NodeType>(&format!("\"{}\"", node_type_str)).unwrap_or(NodeType::Unknown);

        let name = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_NAME).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        let description = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_DESCRIPTION).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            });

        let subtype = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_SUBTYPE).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            });

        let embedding_id = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_EMBEDDING_ID).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    let s = s.to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            });

        let created_at = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_CREATED_AT).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    s.as_str().parse::<DateTime<Utc>>().ok()
                } else {
                    None
                }
            })
            .unwrap_or_else(Utc::now);

        let updated_at = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_UPDATED_AT).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    s.as_str().parse::<DateTime<Utc>>().ok()
                } else {
                    None
                }
            })
            .unwrap_or_else(Utc::now);

        let version = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_VERSION).unwrap())
            .and_then(|v| {
                if let Value::Int(i) = v {
                    Some(*i as u32)
                } else {
                    None
                }
            })
            .unwrap_or(1);

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

    /// Build a `GraphEdge` from SeleneDB edge properties.
    fn build_edge_from_props(
        _edge_id: EdgeId,
        from_uuid: &str,
        to_uuid: &str,
        props: &selene_db_core::property_map::PropertyMap,
    ) -> Option<GraphEdge> {
        let uuid = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_UUID).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.to_string())
                } else {
                    None
                }
            })?;

        let edge_type_str = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_EDGE_TYPE).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.to_string())
                } else {
                    None
                }
            })?;

        let edge_type =
            serde_json::from_str::<RelationshipType>(&format!("\"{}\"", edge_type_str)).unwrap_or(RelationshipType::Unknown);

        let weight = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_WEIGHT).unwrap())
            .and_then(|v| {
                if let Value::Float(f) = v {
                    Some(*f)
                } else if let Value::Int(i) = v {
                    Some(*i as f64)
                } else {
                    None
                }
            });

        let created_at = props
            .get(&selene_db_core::db_string::DbString::try_from(PROP_CREATED_AT).unwrap())
            .and_then(|v| {
                if let Value::String(s) = v {
                    s.as_str().parse::<DateTime<Utc>>().ok()
                } else {
                    None
                }
            })
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
            from_id: from_uuid.to_string(),
            to_id: to_uuid.to_string(),
            properties,
            created_at,
            weight,
        })
    }

    /// Query nodes by filter.
    fn query_nodes(&self, filter: NodeFilter) -> Vec<GraphNode> {
        let snapshot = self.graph_db.shared_graph().read();
        let mut results = Vec::new();

        if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_SPIRE_NODE).unwrap()) {
            for row in bitmap.iter() {
                if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                    if let Some(props) = snapshot.node_properties(node_id) {
                        if let Some(node) = Self::build_node_from_props(node_id, &props) {
                            // Apply filters
                            if let Some(ref nt) = filter.node_type {
                                if node.node_type != *nt {
                                    continue;
                                }
                            }
                            if let Some(ref st) = filter.subtype {
                                if node.subtype.as_deref() != Some(st.as_str()) {
                                    continue;
                                }
                            }
                            if let Some(ref name) = filter.name {
                                if !node.name.to_lowercase().contains(&name.to_lowercase()) {
                                    continue;
                                }
                            }
                            results.push(node);
                        }
                    }
                }
            }
        }

        results
    }

    /// Get edges for a node (both outgoing and incoming).
    fn get_edges_for_node(&self, node_id: &str) -> Vec<GraphEdge> {
        let snapshot = self.graph_db.shared_graph().read();
        let mut edges = Vec::new();

        // Get the SeleneDB node ID
        let selene_id = match self.uuid_to_node.get(node_id) {
            Some(id) => *id,
            None => return edges,
        };

        // Outgoing edges
        if let Some(adj) = snapshot.outgoing_edges(selene_id) {
            for edge_ref in adj.iter() {
                let eid = edge_ref.edge_id;
                if let Some(props) = snapshot.edge_properties(eid) {
                    if let Some(endpoints) = snapshot.edge_endpoints(eid) {
                        let to_uuid = self
                            .node_to_uuid
                            .get(&endpoints.1)
                            .cloned()
                            .unwrap_or_else(|| endpoints.1.to_string());
                        if let Some(edge) = Self::build_edge_from_props(eid, node_id, &to_uuid, &props) {
                            edges.push(edge);
                        }
                    }
                }
            }
        }

        // Incoming edges
        if let Some(adj) = snapshot.incoming_edges(selene_id) {
            for edge_ref in adj.iter() {
                let eid = edge_ref.edge_id;
                if let Some(props) = snapshot.edge_properties(eid) {
                    if let Some(endpoints) = snapshot.edge_endpoints(eid) {
                        let from_uuid = self
                            .node_to_uuid
                            .get(&endpoints.0)
                            .cloned()
                            .unwrap_or_else(|| endpoints.0.to_string());
                        if let Some(edge) = Self::build_edge_from_props(eid, &from_uuid, node_id, &props) {
                            edges.push(edge);
                        }
                    }
                }
            }
        }

        edges
    }

    /// Check whether adding a `depends_on` edge from `from_id` to `to_id`
    /// would create a cycle. Uses DFS from `to_id` following outgoing edges.
    fn would_create_cycle(&self, from_id: &str, to_id: &str) -> bool {
        if from_id == to_id {
            return true;
        }

        let from_selene = match self.uuid_to_node.get(from_id) {
            Some(id) => *id,
            None => return false,
        };

        let snapshot = self.graph_db.shared_graph().read();

        // DFS from `to_id` following outgoing depends_on edges
        let mut stack = vec![*self.uuid_to_node.get(to_id).unwrap_or(&from_selene)];
        let mut visited = std::collections::HashSet::new();

        while let Some(current) = stack.pop() {
            if current == from_selene {
                return true;
            }
            if !visited.insert(current) {
                continue;
            }
            if let Some(adj) = snapshot.outgoing_edges(current) {
                for edge_ref in adj.iter() {
                    let eid = edge_ref.edge_id;
                    if let Some(label) = snapshot.edge_label(eid) {
                        if label.as_str() == "DependsOn" {
                            if let Some(endpoints) = snapshot.edge_endpoints(eid) {
                                stack.push(endpoints.1);
                            }
                        }
                    }
                }
            }
        }
        false
    }

    /// BFS traversal from a start node, respecting relationship_types and direction filters.
    fn traverse(
        &self,
        start_node_id: &str,
        max_depth: usize,
        max_nodes: usize,
        relationship_types: &Option<Vec<RelationshipType>>,
        direction: &Option<TraversalDirection>,
    ) -> GraphResult {
        let snapshot = self.graph_db.shared_graph().read();
        let mut visited_nodes: Vec<GraphNode> = Vec::new();
        let mut visited_edges: Vec<GraphEdge> = Vec::new();
        let mut visited_ids = std::collections::HashSet::new();

        // Get start node
        if let Some(selene_id) = self.uuid_to_node.get(start_node_id) {
            if let Some(props) = snapshot.node_properties(*selene_id) {
                if let Some(node) = Self::build_node_from_props(*selene_id, &props) {
                    visited_nodes.push(node);
                    visited_ids.insert(start_node_id.to_string());
                }
            }
        }

        let mut current_level = vec![start_node_id.to_string()];
        for _depth in 0..max_depth {
            if visited_nodes.len() >= max_nodes {
                break;
            }
            let mut next_level = Vec::new();
            for node_id in &current_level {
                let selene_id = match self.uuid_to_node.get(node_id) {
                    Some(id) => *id,
                    None => continue,
                };

                // Check outgoing edges
                if let Some(adj) = snapshot.outgoing_edges(selene_id) {
                    for edge_ref in adj.iter() {
                        let eid = edge_ref.edge_id;
                        if let Some(endpoints) = snapshot.edge_endpoints(eid) {
                            let to_uuid = self
                                .node_to_uuid
                                .get(&endpoints.1)
                                .cloned()
                                .unwrap_or_else(|| endpoints.1.to_string());

                            if let Some(props) = snapshot.edge_properties(eid) {
                                if let Some(edge) = Self::build_edge_from_props(eid, node_id, &to_uuid, &props) {
                                    // Filter by relationship type
                                    if let Some(ref types) = relationship_types {
                                        if !types.contains(&edge.edge_type) {
                                            continue;
                                        }
                                    }

                                    // Filter by direction
                                    let matches_direction = match direction {
                                        Some(TraversalDirection::In) => false,
                                        _ => true,
                                    };
                                    if !matches_direction {
                                        continue;
                                    }

                                    if !visited_edges.iter().any(|e| e.id == edge.id) {
                                        visited_edges.push(edge);
                                    }
                                    if visited_ids.insert(to_uuid.clone()) {
                                        if let Some(props) = snapshot.node_properties(endpoints.1) {
                                            if let Some(node) = Self::build_node_from_props(endpoints.1, &props) {
                                                visited_nodes.push(node);
                                                next_level.push(to_uuid);
                                                if visited_nodes.len() >= max_nodes {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Check incoming edges
                if let Some(adj) = snapshot.incoming_edges(selene_id) {
                    for edge_ref in adj.iter() {
                        let eid = edge_ref.edge_id;
                        if let Some(endpoints) = snapshot.edge_endpoints(eid) {
                            let from_uuid = self
                                .node_to_uuid
                                .get(&endpoints.0)
                                .cloned()
                                .unwrap_or_else(|| endpoints.0.to_string());

                            if let Some(props) = snapshot.edge_properties(eid) {
                                if let Some(edge) = Self::build_edge_from_props(eid, &from_uuid, node_id, &props) {
                                    // Filter by relationship type
                                    if let Some(ref types) = relationship_types {
                                        if !types.contains(&edge.edge_type) {
                                            continue;
                                        }
                                    }

                                    // Filter by direction
                                    let matches_direction = match direction {
                                        Some(TraversalDirection::Out) => false,
                                        _ => true,
                                    };
                                    if !matches_direction {
                                        continue;
                                    }

                                    if !visited_edges.iter().any(|e| e.id == edge.id) {
                                        visited_edges.push(edge);
                                    }
                                    if visited_ids.insert(from_uuid.clone()) {
                                        if let Some(props) = snapshot.node_properties(endpoints.0) {
                                            if let Some(node) = Self::build_node_from_props(endpoints.0, &props) {
                                                visited_nodes.push(node);
                                                next_level.push(from_uuid);
                                                if visited_nodes.len() >= max_nodes {
                                                    break;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                if visited_nodes.len() >= max_nodes {
                    break;
                }
            }
            if next_level.is_empty() {
                break;
            }
            current_level = next_level;
        }

        GraphResult {
            total_count: visited_nodes.len(),
            nodes: visited_nodes,
            edges: visited_edges,
        }
    }

    /// Store a node in SeleneDB and return the allocated `NodeId`.
    fn store_in_selene(&mut self, graph_node: &GraphNode) -> Result<NodeId> {
        // Build labels: SpireNode + node_type + optional subtype
        let mut labels = vec![LABEL_SPIRE_NODE.to_string(), format!("{:?}", graph_node.node_type)];
        if let Some(ref subtype) = graph_node.subtype {
            labels.push(subtype.clone());
        }

        // Build properties
        let mut properties: Vec<(String, Value)> = Vec::new();
        properties.push((PROP_UUID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(graph_node.id.clone()).unwrap())));
        properties.push((PROP_NODE_TYPE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(format!("{:?}", graph_node.node_type)).unwrap())));
        properties.push((PROP_NAME.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(graph_node.name.clone()).unwrap())));
        if let Some(ref desc) = graph_node.description {
            properties.push((PROP_DESCRIPTION.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(desc.clone()).unwrap())));
        } else {
            properties.push((PROP_DESCRIPTION.to_string(), Value::String(selene_db_core::db_string::DbString::try_from("").unwrap())));
        }
        if let Some(ref subtype) = graph_node.subtype {
            properties.push((PROP_SUBTYPE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(subtype.clone()).unwrap())));
        } else {
            properties.push((PROP_SUBTYPE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from("").unwrap())));
        }
        if let Some(ref emb_id) = graph_node.embedding_id {
            properties.push((PROP_EMBEDDING_ID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(emb_id.clone()).unwrap())));
        } else {
            properties.push((PROP_EMBEDDING_ID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from("").unwrap())));
        }
        properties.push((PROP_CREATED_AT.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(graph_node.created_at.to_rfc3339()).unwrap())));
        properties.push((PROP_UPDATED_AT.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(graph_node.updated_at.to_rfc3339()).unwrap())));
        properties.push((PROP_VERSION.to_string(), Value::Int(graph_node.version as i64)));

        // Convert JSON properties to SeleneDB values
        for (key, json_val) in &graph_node.properties {
            if let Some(val) = json_value_to_selene(json_val) {
                properties.push((key.clone(), val));
            }
        }

        let selene_id = self.graph_db.create_node(labels, properties)?;
        Ok(selene_id)
    }

    /// Store an edge in SeleneDB and return the allocated `EdgeId`.
    fn store_edge_in_selene(
        &mut self,
        from_uuid: &str,
        predicate: &str,
        to_uuid: &str,
        properties: &[(String, Value)],
    ) -> Result<EdgeId> {
        let from_id = self
            .uuid_to_node
            .get(from_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Source node not found: {}", from_uuid))?;
        let to_id = self
            .uuid_to_node
            .get(to_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Target node not found: {}", to_uuid))?;

        let edge_id = self.graph_db.create_edge(predicate, from_id, to_id, properties.to_vec())?;
        Ok(edge_id)
    }
}

#[async_trait]
impl Actor for MemoryGraphActor {
    type Message = MemoryGraphMessage;

    async fn handle(&mut self, msg: Self::Message) {
        match msg {
            // ── GetNode ─────────────────────────────────
            MemoryGraphMessage::GetNode { id, reply_to } => {
                info!("MemoryGraph: get_node({})", id);
                let result = match self.uuid_to_node.get(&id) {
                    Some(selene_id) => {
                        let snapshot = self.graph_db.shared_graph().read();
                        match snapshot.node_properties(*selene_id) {
                            Some(props) => Ok(Self::build_node_from_props(*selene_id, &props)),
                            None => Ok(None),
                        }
                    }
                    None => Ok(None),
                };
                let _ = reply_to.send(result);
            }

            // ── QueryNodes ──────────────────────────────
            MemoryGraphMessage::QueryNodes { filter, reply_to } => {
                info!("MemoryGraph: query_nodes");
                let result = Ok(self.query_nodes(filter));
                let _ = reply_to.send(result);
            }

            // ── StoreNode ───────────────────────────────
            MemoryGraphMessage::StoreNode { node, reply_to } => {
                info!("MemoryGraph: store_node({})", node.name);

                // Enforce unique (type, name) constraint
                if self.has_duplicate(&node.node_type, &node.name) {
                    let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::DuplicateNode {
                        type_name: format!("{:?}", node.node_type),
                        name: node.name.clone(),
                    })));
                    return;
                }

                let graph_node = Self::create_node_from_input(node);

                // Store in SeleneDB first to get the allocated ID.
                let selene_id = match self.store_in_selene(&graph_node) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                // Update the UUID ↔ SeleneDB ID cache
                self.uuid_to_node.insert(graph_node.id.clone(), selene_id);
                self.node_to_uuid.insert(selene_id, graph_node.id.clone());

                let _ = reply_to.send(Ok(graph_node));
            }

            // ── UpdateNode ──────────────────────────────
            MemoryGraphMessage::UpdateNode { id, updates, reply_to } => {
                info!("MemoryGraph: update_node({})", id);

                let selene_id = self.uuid_to_node.get(&id).copied();
                let result = match selene_id {
                    Some(selene_id) => {
                        let snapshot = self.graph_db.shared_graph().read();
                        match snapshot.node_properties(selene_id) {
                            Some(props) => {
                                if let Some(existing) = Self::build_node_from_props(selene_id, &props) {
                                    drop(snapshot);
                                    let updated = Self::apply_updates(&existing, updates);

                                    // Delete old node and re-create with updated properties
                                    if let Err(e) = self.graph_db.delete_node(selene_id) {
                                        Err(anyhow::anyhow!("Failed to delete old node: {}", e))
                                    } else {
                                        // Remove old cache entries
                                        self.uuid_to_node.remove(&id);
                                        self.node_to_uuid.remove(&selene_id);

                                        // Store updated node
                                        match self.store_in_selene(&updated) {
                                            Ok(new_id) => {
                                                self.uuid_to_node.insert(updated.id.clone(), new_id);
                                                self.node_to_uuid.insert(new_id, updated.id.clone());
                                                Ok(updated)
                                            }
                                            Err(e) => Err(e),
                                        }
                                    }
                                } else {
                                    Err(anyhow::anyhow!("Failed to deserialize node: {}", id))
                                }
                            }
                            None => Err(anyhow::anyhow!("Node not found: {}", id)),
                        }
                    }
                    None => Err(anyhow::anyhow!("Node not found: {}", id)),
                };

                let _ = reply_to.send(result);
            }

            // ── DeleteNode ──────────────────────────────
            MemoryGraphMessage::DeleteNode { id, reply_to } => {
                info!("MemoryGraph: delete_node({})", id);

                let selene_id = self.uuid_to_node.get(&id).copied();
                let result = match selene_id {
                    Some(selene_id) => {
                        // Delete all edges connected to this node first
                        let edges = self.get_edges_for_node(&id);
                        for edge in &edges {
                            if let Some(edge_id) = self.uuid_to_edge.get(&edge.id).copied() {
                                let _ = self.graph_db.delete_edge(edge_id);
                                self.uuid_to_edge.remove(&edge.id);
                                self.edge_to_uuid.remove(&edge_id);
                            }
                        }

                        match self.graph_db.delete_node(selene_id) {
                            Ok(()) => {
                                self.uuid_to_node.remove(&id);
                                self.node_to_uuid.remove(&selene_id);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    }
                    None => Err(anyhow::anyhow!("Node not found: {}", id)),
                };

                let _ = reply_to.send(result);
            }

            // ── CreateRelationship ──────────────────────
            MemoryGraphMessage::CreateRelationship { rel, reply_to } => {
                info!("MemoryGraph: create_relationship({:?}, {} -> {})", rel.edge_type, rel.from_id, rel.to_id);

                // Validate that both nodes exist
                if !self.uuid_to_node.contains_key(&rel.from_id) {
                    let _ = reply_to.send(Err(anyhow::anyhow!("Source node not found: {}", rel.from_id)));
                    return;
                }
                if !self.uuid_to_node.contains_key(&rel.to_id) {
                    let _ = reply_to.send(Err(anyhow::anyhow!("Target node not found: {}", rel.to_id)));
                    return;
                }

                // Enforce acyclic constraint for depends_on
                if rel.edge_type == RelationshipType::DependsOn {
                    if self.would_create_cycle(&rel.from_id, &rel.to_id) {
                        let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::AcyclicDependencyViolation {
                            from: rel.from_id.clone(),
                            to: rel.to_id.clone(),
                        })));
                        return;
                    }
                }

                let now = Utc::now();
                let edge_uuid = Uuid::new_v4().to_string();

                let mut properties: Vec<(String, Value)> = Vec::new();
                properties.push((PROP_UUID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(edge_uuid.clone()).unwrap())));
                properties.push((PROP_EDGE_TYPE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(format!("{:?}", rel.edge_type)).unwrap())));
                properties.push((PROP_CREATED_AT.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(now.to_rfc3339()).unwrap())));
                if let Some(weight) = rel.weight {
                    properties.push((PROP_WEIGHT.to_string(), Value::Float(weight)));
                }
                if let Some(ref extra_props) = rel.properties {
                    for (key, json_val) in extra_props {
                        if let Some(val) = json_value_to_selene(json_val) {
                            properties.push((key.clone(), val));
                        }
                    }
                }

                let predicate = format!("{:?}", rel.edge_type);
                let edge_id = match self.store_edge_in_selene(&rel.from_id, &predicate, &rel.to_id, &properties) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                self.uuid_to_edge.insert(edge_uuid.clone(), edge_id);
                self.edge_to_uuid.insert(edge_id, edge_uuid.clone());

                let graph_edge = GraphEdge {
                    id: edge_uuid,
                    edge_type: rel.edge_type,
                    from_id: rel.from_id,
                    to_id: rel.to_id,
                    properties: rel.properties.unwrap_or_default(),
                    created_at: now,
                    weight: rel.weight,
                };

                let _ = reply_to.send(Ok(graph_edge));
            }

            // ── GetRelationships ────────────────────────
            MemoryGraphMessage::GetRelationships { node_id, reply_to } => {
                info!("MemoryGraph: get_relationships({})", node_id);
                let result = Ok(self.get_edges_for_node(&node_id));
                let _ = reply_to.send(result);
            }

            // ── DeleteRelationship ──────────────────────
            MemoryGraphMessage::DeleteRelationship { id, reply_to } => {
                info!("MemoryGraph: delete_relationship({})", id);

                let edge_id = self.uuid_to_edge.get(&id).copied();
                let result = match edge_id {
                    Some(edge_id) => {
                        match self.graph_db.delete_edge(edge_id) {
                            Ok(()) => {
                                self.uuid_to_edge.remove(&id);
                                self.edge_to_uuid.remove(&edge_id);
                                Ok(())
                            }
                            Err(e) => Err(e),
                        }
                    }
                    None => Err(anyhow::anyhow!("Relationship not found: {}", id)),
                };

                let _ = reply_to.send(result);
            }

            // ── Traverse ────────────────────────────────
            MemoryGraphMessage::Traverse { start_node_id, options, reply_to } => {
                info!("MemoryGraph: traverse({})", start_node_id);

                let max_depth = options.max_depth as usize;
                let max_nodes = options.max_nodes.unwrap_or(100);
                let result = self.traverse(
                    &start_node_id,
                    max_depth,
                    max_nodes,
                    &options.relationship_types,
                    &options.direction,
                );

                let _ = reply_to.send(Ok(TraversalResult {
                    nodes: result.nodes,
                    edges: result.edges,
                    paths: Vec::new(), // Path construction is expensive; skip for now
                }));
            }

            // ── GetProjectContext ───────────────────────
            MemoryGraphMessage::GetProjectContext { reply_to } => {
                info!("MemoryGraph: get_project_context");

                let snapshot = self.graph_db.shared_graph().read();
                let mut project: Option<GraphNode> = None;
                let mut active_context: Option<GraphNode> = None;
                let mut milestones: Vec<GraphNode> = Vec::new();
                let mut blockers: Vec<GraphNode> = Vec::new();
                let mut decisions: Vec<GraphNode> = Vec::new();
                let mut entities: Vec<GraphNode> = Vec::new();
                let mut standards: Vec<GraphNode> = Vec::new();

                if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_SPIRE_NODE).unwrap()) {
                    for row in bitmap.iter() {
                        if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                            if let Some(props) = snapshot.node_properties(node_id) {
                                if let Some(node) = Self::build_node_from_props(node_id, &props) {
                                    match node.node_type {
                                        NodeType::Project => project = Some(node),
                                        NodeType::ActiveContext => active_context = Some(node),
                                        NodeType::Milestone => milestones.push(node),
                                        NodeType::Blocker => blockers.push(node),
                                        NodeType::Decision => decisions.push(node),
                                        NodeType::Entity => entities.push(node),
                                        NodeType::Standard => standards.push(node),
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                }

                let total_nodes = snapshot.node_count();
                let total_edges = snapshot.edge_count();

                let result = ProjectSnapshot {
                    project: project.unwrap_or_else(|| GraphNode {
                        id: String::new(),
                        node_type: NodeType::Project,
                        subtype: None,
                        name: "Unknown".to_string(),
                        description: None,
                        properties: HashMap::new(),
                        embedding_id: None,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        version: 0,
                    }),
                    active_context,
                    milestones,
                    blockers,
                    recent_decisions: decisions,
                    recent_entities: entities,
                    standards,
                    stats: ProjectStats {
                        total_nodes,
                        total_relationships: total_edges,
                        last_updated: Utc::now(),
                    },
                };

                let _ = reply_to.send(Ok(result));
            }

            // ── SearchContext ───────────────────────────
            MemoryGraphMessage::SearchContext { query, options, reply_to } => {
                info!("MemoryGraph: search_context({})", query);

                let opts = options.unwrap_or_default();
                let top_k = opts.top_k.unwrap_or(10);
                let threshold = opts.threshold.unwrap_or(0.5);

                // Generate embedding for the query
                let query_embedding = match self.embedder.embed(&query).await {
                    Ok(emb) => emb,
                    Err(e) => {
                        let _ = reply_to.send(Err(anyhow::anyhow!("Embedding failed: {}", e)));
                        return;
                    }
                };

                // Perform vector search
                let hits = match self.graph_db.exact_vector_search(
                    LABEL_SPIRE_NODE,
                    "embedding",
                    query_embedding.vector,
                    VectorMetric::Cosine,
                    top_k,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                let mut scored_nodes: Vec<ScoredNode> = Vec::new();
                let snapshot = self.graph_db.shared_graph().read();

                for hit in &hits {
                    let similarity = hit.distance as f64;
                    if similarity < threshold {
                        continue;
                    }

                    if let Some(props) = snapshot.node_properties(hit.node_id) {
                        if let Some(node) = Self::build_node_from_props(hit.node_id, &props) {
                            // Filter by node type if specified
                            if let Some(ref node_types) = opts.node_types {
                                if !node_types.contains(&node.node_type) {
                                    continue;
                                }
                            }

                            scored_nodes.push(ScoredNode {
                                node,
                                similarity,
                                source: RetrievalSource::Semantic,
                                score: similarity,
                            });
                        }
                    }
                }

                // Sort by similarity descending
                scored_nodes.sort_by(|a, b| b.similarity.partial_cmp(&a.similarity).unwrap_or(std::cmp::Ordering::Equal));

                // Collect relationships between returned nodes
                let node_ids: std::collections::HashSet<String> =
                    scored_nodes.iter().map(|sn| sn.node.id.clone()).collect();
                let mut relationships: Vec<GraphEdge> = Vec::new();

                for sn in &scored_nodes {
                    let edges = self.get_edges_for_node(&sn.node.id);
                    for edge in edges {
                        if node_ids.contains(&edge.from_id) && node_ids.contains(&edge.to_id) {
                            if !relationships.iter().any(|r| r.id == edge.id) {
                                relationships.push(edge);
                            }
                        }
                    }
                }

                let truncated = scored_nodes.len() > top_k;
                scored_nodes.truncate(top_k);

                let result = ContextSearchResult {
                    nodes: scored_nodes,
                    relationships,
                    total_results: hits.len(),
                    search_time_ms: 0,
                    truncated,
                };

                let _ = reply_to.send(Ok(result));
            }

            // ── AddMemory ───────────────────────────────
            MemoryGraphMessage::AddMemory { text, metadata, reply_to } => {
                info!("MemoryGraph: add_memory");

                // Generate embedding for the memory text
                let _embedding = match self.embedder.embed(&text).await {
                    Ok(emb) => emb,
                    Err(e) => {
                        let _ = reply_to.send(Err(anyhow::anyhow!("Embedding failed: {}", e)));
                        return;
                    }
                };

                let now = Utc::now();
                let mem_id = Uuid::new_v4().to_string();
                let emb_id = format!("emb_{}", mem_id);

                let meta = metadata.unwrap_or(MemoryMetadata {
                    mem_type: None,
                    tags: None,
                    source: None,
                    confidence: None,
                });

                // Store as a node in SeleneDB
                let mut properties: Vec<(String, Value)> = Vec::new();
                properties.push((PROP_UUID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(mem_id.clone()).unwrap())));
                properties.push((PROP_NODE_TYPE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from("Memory").unwrap())));
                properties.push((PROP_NAME.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(text.clone()).unwrap())));
                properties.push((PROP_EMBEDDING_ID.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(emb_id.clone()).unwrap())));
                properties.push((PROP_CREATED_AT.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(now.to_rfc3339()).unwrap())));
                properties.push((PROP_UPDATED_AT.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(now.to_rfc3339()).unwrap())));
                properties.push((PROP_VERSION.to_string(), Value::Int(1)));

                // Store metadata as JSON
                if let Ok(json_str) = serde_json::to_string(&meta) {
                    properties.push(("memory_metadata".to_string(), Value::String(selene_db_core::db_string::DbString::try_from(json_str).unwrap())));
                }

                let labels = vec![LABEL_SPIRE_NODE.to_string(), "Memory".to_string()];
                let selene_id = match self.graph_db.create_node(labels, properties) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                self.uuid_to_node.insert(mem_id.clone(), selene_id);
                self.node_to_uuid.insert(selene_id, mem_id.clone());

                // Store the embedding for vector search
                // (Vector index must be created separately via create_vector_index)

                let entry = MemoryEntry {
                    id: mem_id.clone(),
                    text,
                    embedding_id: emb_id,
                    metadata: meta,
                    node_id: Some(mem_id),
                    created_at: now,
                    updated_at: now,
                };

                let _ = reply_to.send(Ok(entry));
            }

            // ── Recall ──────────────────────────────────
            MemoryGraphMessage::Recall { query, limit, reply_to } => {
                info!("MemoryGraph: recall({})", query);

                let limit = limit.unwrap_or(10);

                // Generate embedding for the query
                let query_embedding = match self.embedder.embed(&query).await {
                    Ok(emb) => emb,
                    Err(e) => {
                        let _ = reply_to.send(Err(anyhow::anyhow!("Embedding failed: {}", e)));
                        return;
                    }
                };

                // Perform vector search on Memory nodes
                let hits = match self.graph_db.exact_vector_search(
                    "Memory",
                    "embedding",
                    query_embedding.vector,
                    VectorMetric::Cosine,
                    limit,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return;
                    }
                };

                let mut entries: Vec<MemoryEntry> = Vec::new();
                let snapshot = self.graph_db.shared_graph().read();

                for hit in &hits {
                    if let Some(props) = snapshot.node_properties(hit.node_id) {
                        let uuid = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_UUID).unwrap())
                            .and_then(|v| if let Value::String(s) = v { Some(s.to_string()) } else { None })
                            .unwrap_or_default();

                        let text = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_NAME).unwrap())
                            .and_then(|v| if let Value::String(s) = v { Some(s.to_string()) } else { None })
                            .unwrap_or_default();

                        let emb_id = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_EMBEDDING_ID).unwrap())
                            .and_then(|v| if let Value::String(s) = v { Some(s.to_string()) } else { None })
                            .unwrap_or_default();

                        let created_at = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_CREATED_AT).unwrap())
                            .and_then(|v| if let Value::String(s) = v { s.as_str().parse::<DateTime<Utc>>().ok() } else { None })
                            .unwrap_or_else(Utc::now);

                        let updated_at = props
                            .get(&selene_db_core::db_string::DbString::try_from(PROP_UPDATED_AT).unwrap())
                            .and_then(|v| if let Value::String(s) = v { s.as_str().parse::<DateTime<Utc>>().ok() } else { None })
                            .unwrap_or_else(Utc::now);

                        let metadata = props
                            .get(&selene_db_core::db_string::DbString::try_from("memory_metadata").unwrap())
                            .and_then(|v| {
                                if let Value::String(s) = v {
                                    serde_json::from_str::<MemoryMetadata>(s.as_str()).ok()
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(MemoryMetadata {
                                mem_type: None,
                                tags: None,
                                source: None,
                                confidence: None,
                            });

                        entries.push(MemoryEntry {
                            id: uuid,
                            text,
                            embedding_id: emb_id,
                            metadata,
                            node_id: Some(hit.node_id.to_string()),
                            created_at,
                            updated_at,
                        });
                    }
                }

                let _ = reply_to.send(Ok(entries));
            }

            // ── SetConfig ───────────────────────────────
            MemoryGraphMessage::SetConfig { key, value, reply_to } => {
                info!("MemoryGraph: set_config({})", key);

                let result = (|| -> Result<()> {
                    let json_str = serde_json::to_string(&value)
                        .map_err(|e| anyhow::anyhow!("Failed to serialize config value: {}", e))?;

                    // Check if config key already exists
                    let snapshot = self.graph_db.shared_graph().read();
                    let existing = if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_CONFIG).unwrap()) {
                        let mut found = None;
                        for row in bitmap.iter() {
                            if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                                if let Some(props) = snapshot.node_properties(node_id) {
                                    if let Some(key_val) = props.get(&selene_db_core::db_string::DbString::try_from(PROP_NAME).unwrap()) {
                                        if let Value::String(s) = key_val {
                                            if s.as_str() == key {
                                                found = Some(node_id);
                                                break;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        found
                    } else {
                        None
                    };
                    drop(snapshot);

                    if let Some(node_id) = existing {
                        // Update existing config node
                        self.graph_db.delete_node(node_id)?;
                    }

                    // Create new config node
                    let properties = vec![
                        (PROP_NAME.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(key.clone()).unwrap())),
                        (PROP_CONFIG_VALUE.to_string(), Value::String(selene_db_core::db_string::DbString::try_from(json_str).unwrap())),
                    ];
                    self.graph_db.create_node(vec![LABEL_CONFIG.to_string()], properties)?;

                    Ok(())
                })();

                let _ = reply_to.send(result);
            }

            // ── GetConfig ───────────────────────────────
            MemoryGraphMessage::GetConfig { key, reply_to } => {
                info!("MemoryGraph: get_config({})", key);

                let result = (|| -> Result<Option<serde_json::Value>> {
                    let snapshot = self.graph_db.shared_graph().read();
                    if let Some(bitmap) = snapshot.nodes_with_label(&selene_db_core::db_string::DbString::try_from(LABEL_CONFIG).unwrap()) {
                        for row in bitmap.iter() {
                            if let Some(node_id) = snapshot.node_id_for_row(selene_db_graph::store::RowIndex::new(row)) {
                                if let Some(props) = snapshot.node_properties(node_id) {
                                    if let Some(key_val) = props.get(&selene_db_core::db_string::DbString::try_from(PROP_NAME).unwrap()) {
                                        if let Value::String(s) = key_val {
                                            if s.as_str() == key {
                                                if let Some(val_val) = props.get(&selene_db_core::db_string::DbString::try_from(PROP_CONFIG_VALUE).unwrap()) {
                                                    if let Value::String(json_str) = val_val {
                                                        let value: serde_json::Value = serde_json::from_str(json_str.as_str())
                                                            .map_err(|e| anyhow::anyhow!("Failed to deserialize config: {}", e))?;
                                                        return Ok(Some(value));
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(None)
                })();

                let _ = reply_to.send(result);
            }

            // ── Sync ────────────────────────────────────
            MemoryGraphMessage::Sync { reply_to } => {
                info!("MemoryGraph: sync");
                // With SeleneDB, data is already persisted via WAL.
                // This is a no-op but we compact to reclaim space.
                let result = self.graph_db.compact();
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
        serde_json::Value::String(s) => Some(Value::String(selene_db_core::db_string::DbString::try_from(s.clone()).unwrap())),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().filter_map(json_value_to_selene).collect();
            Some(Value::List(items))
        }
        serde_json::Value::Object(obj) => {
            use selene_db_core::value::Record;
            let mut fields = Vec::new();
            for (k, v) in obj {
                if let Some(val) = json_value_to_selene(v) {
                    fields.push((selene_db_core::db_string::DbString::try_from(k.clone()).unwrap(), val));
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
                _ => {} // non-exhaustive enum; ignore other variants
            }
            Some(serde_json::Value::Object(obj))
        }
        _ => None,
    }
}
