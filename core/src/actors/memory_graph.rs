//! MemoryGraphActor — backed by SeleneDB's `GraphDb`.
//!
//! This actor is the sole data store for the system, owning graph nodes, edges,
//! and vector embeddings. All storage is delegated to `GraphDb` (SeleneDB),
//! which provides lock-free reads, serialized writes, and optional WAL persistence.
//!
//! # ID Mapping
//!
//! The external API uses UUID-based `String` IDs (for compatibility with the
//! TypeScript extension), while SeleneDB uses compact `u64` IDs (`NodeId`/`EdgeId`).
//! This actor maintains a bidirectional mapping between the two.

use anyhow::Result;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tonari_actor::{Actor, Context};
use tracing::info;
use uuid::Uuid;

use crate::graph::GraphDb;
use crate::models::embedding::Embedder;
use crate::models::graph::GraphResult;
use crate::models::memory_graph::{
    ContextSearchResult, GraphEdge, GraphNode, MemoryEntry, MemoryMetadata, NodeFilter, NodeInput,
    NodeType, NodeUpdate, ProjectSnapshot, ProjectStats, RelationshipInput, RelationshipType,
    RetrievalSource, SchemaError, ScoredNode, SearchOptions, TraversalDirection, TraversalOptions,
    TraversalResult,
};

use selene_core::identity::{EdgeId, NodeId};
use selene_core::value::Value;
use selene_core::vector::VectorMetric;

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
/// No separate GraphActor or VectorActor — all operations are handled inline.
///
/// Enforces schema constraints:
/// - Unique `(type, name)` per node
/// - Referential integrity for relationships (from_id / to_id must exist)
/// - Acyclic `depends_on` relationships
pub struct MemoryGraphActor {
    /// The SeleneDB-backed graph database.
    graph_db: Arc<GraphDb>,

    /// UUID → SeleneDB NodeId mapping.
    /// The external API uses UUID strings; SeleneDB uses compact u64 IDs.
    uuid_to_node: HashMap<String, NodeId>,

    /// UUID → SeleneDB EdgeId mapping.
    uuid_to_edge: HashMap<String, EdgeId>,

    /// Reverse mapping: SeleneDB NodeId → UUID.
    node_to_uuid: HashMap<NodeId, String>,

    /// Reverse mapping: SeleneDB EdgeId → UUID.
    edge_to_uuid: HashMap<EdgeId, String>,

    /// Full GraphNode metadata (properties, timestamps, etc.) keyed by UUID.
    /// SeleneDB stores labels and properties, but we keep the richer
    /// `GraphNode` metadata here for compatibility with the existing API.
    node_meta: HashMap<String, GraphNode>,

    /// Full GraphEdge metadata keyed by UUID.
    edge_meta: HashMap<String, GraphEdge>,

    /// Embedder for text → vector generation.
    embedder: Arc<dyn Embedder>,
}

impl MemoryGraphActor {
    pub fn new(graph_db: Arc<GraphDb>, embedder: Arc<dyn Embedder>) -> Self {
        Self {
            graph_db,
            uuid_to_node: HashMap::new(),
            uuid_to_edge: HashMap::new(),
            node_to_uuid: HashMap::new(),
            edge_to_uuid: HashMap::new(),
            node_meta: HashMap::new(),
            edge_meta: HashMap::new(),
            embedder,
        }
    }

    /// Check whether a node with the given `(type, name)` already exists.
    fn has_duplicate(&self, node_type: &NodeType, name: &str) -> bool {
        self.node_meta
            .values()
            .any(|n| n.node_type == *node_type && n.name == name)
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

    /// Query nodes by filter.
    fn query_nodes(&self, filter: NodeFilter) -> Vec<GraphNode> {
        self.node_meta
            .values()
            .filter(|n| {
                if let Some(ref nt) = filter.node_type {
                    if n.node_type != *nt {
                        return false;
                    }
                }
                if let Some(ref st) = filter.subtype {
                    if n.subtype.as_deref() != Some(st.as_str()) {
                        return false;
                    }
                }
                if let Some(ref name) = filter.name {
                    if !n.name.to_lowercase().contains(&name.to_lowercase()) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    }

    /// Get edges for a node (both outgoing and incoming).
    fn get_edges_for_node(&self, node_id: &str) -> Vec<GraphEdge> {
        self.edge_meta
            .values()
            .filter(|e| e.from_id == node_id || e.to_id == node_id)
            .cloned()
            .collect()
    }

    /// Check whether adding a `depends_on` edge from `from_id` to `to_id`
    /// would create a cycle. Uses DFS from `to_id` following outgoing edges.
    fn would_create_cycle(&self, from_id: &str, to_id: &str) -> bool {
        if from_id == to_id {
            return true;
        }
        // DFS from `to_id` following outgoing depends_on edges
        let mut stack = vec![to_id.to_string()];
        let mut visited = std::collections::HashSet::new();
        while let Some(current) = stack.pop() {
            if current == from_id {
                return true;
            }
            if !visited.insert(current.clone()) {
                continue;
            }
            for edge in self.edge_meta.values() {
                if edge.edge_type == RelationshipType::DependsOn && edge.from_id == current {
                    stack.push(edge.to_id.clone());
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
        let mut visited_nodes: Vec<GraphNode> = Vec::new();
        let mut visited_edges: Vec<GraphEdge> = Vec::new();
        let mut visited_ids = std::collections::HashSet::new();

        if let Some(start) = self.node_meta.get(start_node_id) {
            visited_nodes.push(start.clone());
            visited_ids.insert(start_node_id.to_string());
        }

        let mut current_level = vec![start_node_id.to_string()];
        for _depth in 0..max_depth {
            if visited_nodes.len() >= max_nodes {
                break;
            }
            let mut next_level = Vec::new();
            for node_id in &current_level {
                for edge in self.edge_meta.values() {
                    // Filter by relationship type
                    if let Some(ref types) = relationship_types {
                        if !types.contains(&edge.edge_type) {
                            continue;
                        }
                    }

                    // Determine if this edge matches the requested direction
                    let is_outgoing = edge.from_id == *node_id;
                    let is_incoming = edge.to_id == *node_id;
                    let matches_direction = match direction {
                        Some(TraversalDirection::Out) => is_outgoing,
                        Some(TraversalDirection::In) => is_incoming,
                        Some(TraversalDirection::Both) | None => is_outgoing || is_incoming,
                    };
                    if !matches_direction {
                        continue;
                    }

                    if !visited_edges.iter().any(|e| e.id == edge.id) {
                        visited_edges.push(edge.clone());
                    }
                    let neighbor_id = if is_outgoing {
                        &edge.to_id
                    } else {
                        &edge.from_id
                    };
                    if visited_ids.insert(neighbor_id.clone()) {
                        if let Some(neighbor) = self.node_meta.get(neighbor_id) {
                            visited_nodes.push(neighbor.clone());
                            next_level.push(neighbor_id.clone());
                            if visited_nodes.len() >= max_nodes {
                                break;
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
        let mut txn = self.graph_db.begin_write();

        // Build labels from node_type and optional subtype.
        let mut labels = vec![format!("{:?}", graph_node.node_type)];
        if let Some(ref subtype) = graph_node.subtype {
            labels.push(subtype.clone());
        }

        // Build properties.
        let mut properties: Vec<(String, Value)> = Vec::new();
        properties.push(("name".to_string(), Value::String(graph_node.name.clone().into())));
        if let Some(ref desc) = graph_node.description {
            properties.push(("description".to_string(), Value::String(desc.clone().into())));
        }
        if let Some(ref emb_id) = graph_node.embedding_id {
            properties.push(("embedding_id".to_string(), Value::String(emb_id.clone().into())));
        }
        // Store timestamps as ISO strings.
        properties.push(("created_at".to_string(), Value::String(graph_node.created_at.to_rfc3339().into())));
        properties.push(("updated_at".to_string(), Value::String(graph_node.updated_at.to_rfc3339().into())));
        properties.push(("version".to_string(), Value::Int(graph_node.version as i64)));

        // Convert JSON properties to SeleneDB values.
        for (key, json_val) in &graph_node.properties {
            if let Some(val) = json_value_to_selene(json_val) {
                properties.push((key.clone(), val));
            }
        }

        // Use the crate::graph::Node type for insertion.
        let node = crate::models::graph::Node {
            id: NodeId::TOMBSTONE,
            labels,
            properties,
        };

        let selene_id = txn.insert_node(node)?;
        txn.commit()?;

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
        let from_id = self.uuid_to_node.get(from_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Source node not found: {}", from_uuid))?;
        let to_id = self.uuid_to_node.get(to_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Target node not found: {}", to_uuid))?;

        let edge_id = self.graph_db.insert_edge(from_id, predicate, to_id, properties.to_vec())?;
        Ok(edge_id)
    }
}

impl Actor for MemoryGraphActor {
    type Message = MemoryGraphMessage;
    type Error = anyhow::Error;
    type Context = Context<Self::Message>;

    fn handle(
        &mut self,
        _ctx: &mut Self::Context,
        msg: Self::Message,
    ) -> Result<(), Self::Error> {
        match msg {
            // ── GetNode ─────────────────────────────────
            MemoryGraphMessage::GetNode { id, reply_to } => {
                info!("MemoryGraph: get_node({})", id);
                let result = Ok(self.node_meta.get(&id).cloned());
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
                    return Ok(());
                }

                let mut graph_node = Self::create_node_from_input(node);

                // Store in SeleneDB first to get the allocated ID.
                let selene_id = match self.store_in_selene(&graph_node) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return Ok(());
                    }
                };

                // Record the UUID ↔ SeleneDB ID mapping.
                let uuid = graph_node.id.clone();
                self.uuid_to_node.insert(uuid.clone(), selene_id);
                self.node_to_uuid.insert(selene_id, uuid.clone());

                // Embedding generation is async — spawn a task for it
                let embedder = self.embedder.clone();
                let node_id = uuid.clone();

                if let Some(desc_text) = graph_node.description.clone() {
                    let embedder = embedder.clone();
                    let node_id = node_id.clone();
                    tokio::spawn(async move {
                        match embedder.embed(&desc_text).await {
                            Ok(emb) => {
                                info!(
                                    "MemoryGraph: generated {}d embedding for node {}",
                                    emb.dimensions, node_id
                                );
                            }
                            Err(e) => {
                                info!("MemoryGraph: embedding failed for node {}: {}", node_id, e);
                            }
                        }
                    });
                    graph_node.embedding_id = Some(Uuid::new_v4().to_string());
                }

                self.node_meta.insert(uuid.clone(), graph_node.clone());
                let _ = reply_to.send(Ok(graph_node));
            }

            // ── UpdateNode ──────────────────────────────
            MemoryGraphMessage::UpdateNode {
                id,
                updates,
                reply_to,
            } => {
                info!("MemoryGraph: update_node({})", id);
                match self.node_meta.get(&id) {
                    Some(existing) => {
                        // Enforce unique (type, name) constraint if type or name is changing
                        let new_type = updates.node_type.as_ref().unwrap_or(&existing.node_type);
                        let new_name = updates.name.as_deref().unwrap_or(&existing.name);
                        if *new_type != existing.node_type || new_name != existing.name {
                            if self.has_duplicate(new_type, new_name) {
                                let _ = reply_to.send(Err(anyhow::anyhow!(
                                    SchemaError::DuplicateNode {
                                        type_name: format!("{:?}", new_type),
                                        name: new_name.to_string(),
                                    }
                                )));
                                return Ok(());
                            }
                        }

                        let mut updated = Self::apply_updates(existing, updates);

                        // If description changed, regenerate embedding
                        if updated.description != existing.description {
                            if let Some(desc_text) = updated.description.clone() {
                                let embedder = self.embedder.clone();
                                let node_id = id.clone();
                                tokio::spawn(async move {
                                    match embedder.embed(&desc_text).await {
                                        Ok(emb) => {
                                            info!(
                                                "MemoryGraph: re-embedded node {} ({}d)",
                                                node_id, emb.dimensions
                                            );
                                        }
                                        Err(e) => {
                                            info!("MemoryGraph: re-embedding failed for node {}: {}", node_id, e);
                                        }
                                    }
                                });
                                updated.embedding_id = Some(Uuid::new_v4().to_string());
                            } else {
                                // Description was cleared — remove embedding reference
                                updated.embedding_id = None;
                            }
                        }

                        self.node_meta.insert(id.clone(), updated.clone());
                        let _ = reply_to.send(Ok(updated));
                    }
                    None => {
                        let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::NodeNotFound {
                            id: id.clone(),
                        })));
                    }
                }
            }

            // ── DeleteNode ──────────────────────────────
            MemoryGraphMessage::DeleteNode { id, reply_to } => {
                info!("MemoryGraph: delete_node({})", id);

                // Remove from SeleneDB if we have a mapping.
                if let Some(selene_id) = self.uuid_to_node.remove(&id) {
                    if let Err(e) = self.graph_db.delete_node(selene_id) {
                        info!("MemoryGraph: failed to delete node from SeleneDB: {}", e);
                    }
                    self.node_to_uuid.remove(&selene_id);
                }

                // Remove associated edges from metadata.
                let edge_ids: Vec<String> = self.edge_meta
                    .values()
                    .filter(|e| e.from_id == id || e.to_id == id)
                    .map(|e| e.id.clone())
                    .collect();
                for eid in &edge_ids {
                    if let Some(selene_eid) = self.uuid_to_edge.remove(eid) {
                        let _ = self.graph_db.delete_edge(selene_eid);
                        self.edge_to_uuid.remove(&selene_eid);
                    }
                    self.edge_meta.remove(eid);
                }

                self.node_meta.remove(&id);
                let _ = reply_to.send(Ok(()));
            }

            // ── CreateRelationship ──────────────────────
            MemoryGraphMessage::CreateRelationship { rel, reply_to } => {
                info!("MemoryGraph: create_relationship({} -> {})", rel.from_id, rel.to_id);

                // Enforce referential integrity: both nodes must exist
                if !self.node_meta.contains_key(&rel.from_id) {
                    let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::NodeNotFound {
                        id: rel.from_id.clone(),
                    })));
                    return Ok(());
                }
                if !self.node_meta.contains_key(&rel.to_id) {
                    let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::NodeNotFound {
                        id: rel.to_id.clone(),
                    })));
                    return Ok(());
                }

                // Enforce acyclic constraint for depends_on relationships
                if rel.edge_type == RelationshipType::DependsOn {
                    if self.would_create_cycle(&rel.from_id, &rel.to_id) {
                        let _ = reply_to.send(Err(anyhow::anyhow!(
                            SchemaError::AcyclicDependencyViolation {
                                from: rel.from_id.clone(),
                                to: rel.to_id.clone(),
                            }
                        )));
                        return Ok(());
                    }
                }

                // Convert properties to SeleneDB values.
                let mut selene_props: Vec<(String, Value)> = Vec::new();
                for (key, json_val) in rel.properties.as_ref().unwrap_or(&HashMap::new()) {
                    if let Some(val) = json_value_to_selene(json_val) {
                        selene_props.push((key.clone(), val));
                    }
                }

                // Store in SeleneDB.
                let predicate = format!("{:?}", rel.edge_type);
                let selene_edge_id = match self.store_edge_in_selene(
                    &rel.from_id,
                    &predicate,
                    &rel.to_id,
                    &selene_props,
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        let _ = reply_to.send(Err(e));
                        return Ok(());
                    }
                };

                let edge = GraphEdge {
                    id: Uuid::new_v4().to_string(),
                    edge_type: rel.edge_type,
                    from_id: rel.from_id,
                    to_id: rel.to_id,
                    properties: rel.properties.unwrap_or_default(),
                    created_at: Utc::now(),
                    weight: rel.weight,
                };
                let uuid = edge.id.clone();

                // Record the UUID ↔ SeleneDB ID mapping.
                self.uuid_to_edge.insert(uuid.clone(), selene_edge_id);
                self.edge_to_uuid.insert(selene_edge_id, uuid.clone());

                self.edge_meta.insert(uuid, edge.clone());
                let _ = reply_to.send(Ok(edge));
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

                // Remove from SeleneDB if we have a mapping.
                if let Some(selene_id) = self.uuid_to_edge.remove(&id) {
                    if let Err(e) = self.graph_db.delete_edge(selene_id) {
                        info!("MemoryGraph: failed to delete edge from SeleneDB: {}", e);
                    }
                    self.edge_to_uuid.remove(&selene_id);
                }

                self.edge_meta.remove(&id);
                let _ = reply_to.send(Ok(()));
            }

            // ── Traverse ────────────────────────────────
            MemoryGraphMessage::Traverse {
                start_node_id,
                options,
                reply_to,
            } => {
                info!("MemoryGraph: traverse({}, depth={})", start_node_id, options.max_depth);
                let graph_result = self.traverse(
                    &start_node_id,
                    options.max_depth as usize,
                    options.max_nodes.unwrap_or(100),
                    &options.relationship_types,
                    &options.direction,
                );
                let traversal = TraversalResult {
                    nodes: graph_result.nodes,
                    edges: graph_result.edges,
                    paths: vec![],
                };
                let _ = reply_to.send(Ok(traversal));
            }

            // ── GetProjectContext ───────────────────────
            MemoryGraphMessage::GetProjectContext { reply_to } => {
                info!("MemoryGraph: get_project_context");
                let project = self
                    .node_meta
                    .values()
                    .find(|n| n.node_type == NodeType::Project)
                    .cloned()
                    .unwrap_or(GraphNode {
                        id: "project-root".to_string(),
                        node_type: NodeType::Project,
                        subtype: None,
                        name: "Untitled Project".to_string(),
                        description: Some("No project context available.".to_string()),
                        properties: HashMap::new(),
                        embedding_id: None,
                        created_at: Utc::now(),
                        updated_at: Utc::now(),
                        version: 1,
                    });

                let snapshot = ProjectSnapshot {
                    project,
                    active_context: None,
                    milestones: vec![],
                    blockers: vec![],
                    recent_decisions: vec![],
                    recent_entities: vec![],
                    standards: vec![],
                    stats: ProjectStats {
                        total_nodes: self.node_meta.len(),
                        total_relationships: self.edge_meta.len(),
                        last_updated: Utc::now(),
                    },
                };
                let _ = reply_to.send(Ok(snapshot));
            }

            // ── SearchContext ───────────────────────────
            MemoryGraphMessage::SearchContext {
                query,
                options,
                reply_to,
            } => {
                info!("MemoryGraph: search_context({})", query);
                let top_k = options.as_ref().and_then(|o| o.top_k).unwrap_or(10);

                // Try vector search first via SeleneDB.
                let mut scored: Vec<ScoredNode> = Vec::new();

                // Attempt semantic search using SeleneDB's vector index.
                if let Ok(embedding) = self.embedder.embed(&query).await {
                    let query_vec: Vec<f32> = embedding.vector.iter().map(|&v| v as f32).collect();
                    match self.graph_db.vector_search(&query_vec, VectorMetric::Cosine, top_k) {
                        Ok(results) => {
                            for (selene_id, similarity) in results {
                                if let Some(uuid) = self.node_to_uuid.get(&selene_id) {
                                    if let Some(node) = self.node_meta.get(uuid) {
                                        scored.push(ScoredNode {
                                            node: node.clone(),
                                            similarity,
                                            source: RetrievalSource::Semantic,
                                            score: similarity,
                                        });
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            info!("MemoryGraph: vector search failed, falling back to text: {}", e);
                        }
                    }
                }

                // Fallback: text-based search if vector search returned nothing.
                if scored.is_empty() {
                    let query_lower = query.to_lowercase();
                    for node in self.node_meta.values() {
                        let matches_name = node.name.to_lowercase().contains(&query_lower);
                        let matches_desc = node.description.as_deref().map_or(false, |d| {
                            d.to_lowercase().contains(&query_lower)
                        });
                        if matches_name || matches_desc {
                            scored.push(ScoredNode {
                                node: node.clone(),
                                similarity: 1.0,
                                source: RetrievalSource::Structural,
                                score: 1.0,
                            });
                        }
                    }
                }

                let total = scored.len();
                scored.truncate(top_k);

                let result = ContextSearchResult {
                    nodes: scored,
                    relationships: vec![],
                    total_results: total,
                    search_time_ms: 0,
                    truncated: total > top_k,
                };
                let _ = reply_to.send(Ok(result));
            }

            // ── AddMemory ───────────────────────────────
            MemoryGraphMessage::AddMemory {
                text,
                metadata,
                reply_to,
            } => {
                info!("MemoryGraph: add_memory ({} chars)", text.len());
                let memory_id = Uuid::new_v4().to_string();
                let now = Utc::now();
                let mem_meta = metadata.unwrap_or(MemoryMetadata {
                    mem_type: Some(NodeType::Conversation),
                    tags: None,
                    source: None,
                    confidence: None,
                });

                let entry = MemoryEntry {
                    id: memory_id.clone(),
                    text: text.clone(),
                    embedding_id: memory_id.clone(),
                    metadata: mem_meta,
                    node_id: None,
                    created_at: now,
                    updated_at: now,
                };

                // Spawn async embedding
                let embedder = self.embedder.clone();
                let mem_id = memory_id.clone();
                let text_clone = text.clone();
                tokio::spawn(async move {
                    match embedder.embed(&text_clone).await {
                        Ok(emb) => {
                            info!("MemoryGraph: generated {}d embedding for memory {}", emb.dimensions, mem_id);
                        }
                        Err(e) => {
                            info!("MemoryGraph: embedding failed for memory {}: {}", mem_id, e);
                        }
                    }
                });

                let _ = reply_to.send(Ok(entry));
            }

            // ── Recall ──────────────────────────────────
            MemoryGraphMessage::Recall {
                query,
                limit,
                reply_to,
            } => {
                info!("MemoryGraph: recall({})", query);
                // Simple text-based recall (no vector index yet)
                let query_lower = query.to_lowercase();
                let entries: Vec<MemoryEntry> = self
                    .node_meta
                    .values()
                    .filter(|n| {
                        n.name.to_lowercase().contains(&query_lower)
                            || n.description.as_deref().map_or(false, |d| d.to_lowercase().contains(&query_lower))
                    })
                    .take(limit.unwrap_or(5))
                    .map(|n| MemoryEntry {
                        id: n.id.clone(),
                        text: n.description.clone().unwrap_or_default(),
                        embedding_id: n.embedding_id.clone().unwrap_or_default(),
                        metadata: MemoryMetadata {
                            mem_type: Some(n.node_type.clone()),
                            tags: None,
                            source: None,
                            confidence: None,
                        },
                        node_id: Some(n.id.clone()),
                        created_at: n.created_at,
                        updated_at: n.updated_at,
                    })
                    .collect();
                let _ = reply_to.send(Ok(entries));
            }

            // ── Sync ────────────────────────────────────
            MemoryGraphMessage::Sync { reply_to } => {
                info!("MemoryGraph: sync");
                let _ = reply_to.send(Ok(()));
            }
        }
        Ok(())
    }
}

// ============================================================================
// Helper: Convert serde_json::Value → selene_core::Value
// ============================================================================

/// Convert a `serde_json::Value` to a `selene_core::value::Value`.
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
        serde_json::Value::String(s) => Some(Value::String(s.clone().into())),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.iter().filter_map(json_value_to_selene).collect();
            Some(Value::List(items))
        }
        serde_json::Value::Object(map) => {
            let items: Vec<(String, Value)> = map
                .iter()
                .filter_map(|(k, v)| json_value_to_selene(v).map(|sv| (k.clone(), sv)))
                .collect();
            // Store as a list of key-value pairs since SeleneDB doesn't have a Map type.
            Some(Value::List(
                items
                    .into_iter()
                    .map(|(k, v)| {
                        Value::List(vec![Value::String(k.into()), v])
                    })
                    .collect(),
            ))
        }
    }
}
