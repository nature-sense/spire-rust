// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

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
    AgentContext, ContextSearchResult, GraphEdge, GraphNode, MemoryEntry, MemoryMetadata,
    NodeFilter, NodeInput, NodeType, NodeUpdate, ProjectSnapshot, ProjectStats, RelationshipInput,
    RelationshipType, RetrievalSource, SchemaError, ScoredNode, SearchOptions, TraversalDirection,
    TraversalOptions, TraversalResult,
};


use selene_core::db_string::DbString;
use selene_core::identity::{EdgeId, NodeId};
use selene_core::value::Value;

// ============================================================================
// MemoryGraphMessage Enum — 14 variants matching IMemoryGraph API
// ============================================================================

/// Messages for the MemoryGraph actor.
///
/// This actor is the sole data store for the system, owning graph nodes, edges,
/// and vector embeddings directly (no separate GraphActor or VectorActor).
#[allow(dead_code)]
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

    // ================================================================
    // Agent Infrastructure Messages
    // ================================================================

    // ── Agent Management ─────────────────────────────────
    CreateAgent {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetAgent {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetAgentByName {
        name: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetActiveAgents {
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetAgentContext {
        agent_id: String,
        goal: String,
        reply_to: tokio::sync::oneshot::Sender<Result<AgentContext>>,
    },

    // ── Tool Management ──────────────────────────────────
    CreateTool {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetTool {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetToolsForAgent {
        agent_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetToolByName {
        name: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },

    // ── Plan Management ──────────────────────────────────
    CreatePlan {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetPlan {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetPlanSteps {
        plan_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetNextStep {
        plan_id: String,
        current_order: u32,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<GraphNode>>>,
    },

    // ── Execution Management ─────────────────────────────
    StartExecution {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    UpdateExecutionStatus {
        id: String,
        status: String,
        result: Option<String>,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },
    GetExecution {
        id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetExecutionHistory {
        agent_id: String,
        limit: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetSuccessfulExecutions {
        agent_id: String,
        limit: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetFailedExecutions {
        agent_id: String,
        limit: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },

    // ── Task Result Management ───────────────────────────
    RecordTaskResult {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetTaskResults {
        execution_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },

    // ── Artifact Management ──────────────────────────────
    RecordArtifact {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetArtifacts {
        execution_id: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    GetLatestArtifact {
        agent_id: String,
        artifact_type: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<GraphNode>>>,
    },

    // ── Error Pattern Management ─────────────────────────
    RecordError {
        input: NodeInput,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphNode>>,
    },
    GetErrorByFingerprint {
        fingerprint: String,
        reply_to: tokio::sync::oneshot::Sender<Result<Option<GraphNode>>>,
    },
    GetSimilarErrors {
        embedding_id: String,
        limit: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<GraphNode>>>,
    },
    LinkErrorToFix {
        error_id: String,
        execution_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<()>>,
    },

    // ── Agent Relationship Management ────────────────────
    CreateUsesTool {
        agent_id: String,
        tool_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateFollowsPlan {
        agent_id: String,
        plan_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateContainsStep {
        plan_id: String,
        step_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreatePrecedes {
        from_step_id: String,
        to_step_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateProduced {
        execution_id: String,
        artifact_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateEncounteredError {
        execution_id: String,
        error_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateResolvedBy {
        error_id: String,
        execution_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreatePartOfExecution {
        task_result_id: String,
        execution_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateExecutedBy {
        execution_id: String,
        agent_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
    },
    CreateLearnedFrom {
        agent_id: String,
        execution_id: String,
        properties: HashMap<String, serde_json::Value>,
        reply_to: tokio::sync::oneshot::Sender<Result<GraphEdge>>,
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
#[allow(dead_code)]
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

    /// Check whether adding a `precedes` edge from `from_id` to `to_id`
    /// would create a cycle. Uses DFS from `to_id` following outgoing precedes edges.
    fn would_create_precedes_cycle(&self, from_id: &str, to_id: &str) -> bool {
        if from_id == to_id {
            return true;
        }
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
                if edge.edge_type == RelationshipType::Precedes && edge.from_id == current {
                    stack.push(edge.to_id.clone());
                }
            }
        }
        false
    }

    /// Store a node (with duplicate check) and return the created GraphNode.
    fn handle_store_node(&mut self, input: NodeInput) -> Result<GraphNode> {
        // Enforce unique (type, name) constraint
        if self.has_duplicate(&input.node_type, &input.name) {
            return Err(anyhow::anyhow!(SchemaError::DuplicateNode {
                type_name: format!("{:?}", input.node_type),
                name: input.name.clone(),
            }));
        }

        let mut graph_node = Self::create_node_from_input(input);

        // Store in SeleneDB first to get the allocated ID.
        let selene_id = self.store_in_selene(&graph_node)?;

        // Record the UUID ↔ SeleneDB ID mapping.
        let uuid = graph_node.id.clone();
        self.uuid_to_node.insert(uuid.clone(), selene_id);
        self.node_to_uuid.insert(selene_id, uuid.clone());

        // Embedding generation is deferred — just mark the node for later embedding
        if graph_node.description.is_some() {
            graph_node.embedding_id = Some(Uuid::new_v4().to_string());
        }


        self.node_meta.insert(uuid.clone(), graph_node.clone());
        Ok(graph_node)
    }

    /// Create a relationship (with referential integrity and acyclic checks) and return the created GraphEdge.
    fn handle_create_relationship(&mut self, rel: RelationshipInput) -> Result<GraphEdge> {
        // Enforce referential integrity: both nodes must exist
        if !self.node_meta.contains_key(&rel.from_id) {
            return Err(anyhow::anyhow!(SchemaError::NodeNotFound {
                id: rel.from_id.clone(),
            }));
        }
        if !self.node_meta.contains_key(&rel.to_id) {
            return Err(anyhow::anyhow!(SchemaError::NodeNotFound {
                id: rel.to_id.clone(),
            }));
        }

        // Enforce acyclic constraint for depends_on relationships
        if rel.edge_type == RelationshipType::DependsOn {
            if self.would_create_cycle(&rel.from_id, &rel.to_id) {
                return Err(anyhow::anyhow!(
                    SchemaError::AcyclicDependencyViolation {
                        from: rel.from_id.clone(),
                        to: rel.to_id.clone(),
                    }
                ));
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
        let selene_edge_id = self.store_edge_in_selene(
            &rel.from_id,
            &predicate,
            &rel.to_id,
            &selene_props,
        )?;

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
        Ok(edge)
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
        // Build labels from node_type and optional subtype.
        let mut labels = vec![format!("{:?}", graph_node.node_type)];
        if let Some(ref subtype) = graph_node.subtype {
            labels.push(subtype.clone());
        }

        // Build properties.
        let mut properties: Vec<(String, Value)> = Vec::new();
        properties.push(("name".to_string(), Value::String(DbString::from_string(graph_node.name.clone()).unwrap())));
        if let Some(ref desc) = graph_node.description {
            properties.push(("description".to_string(), Value::String(DbString::from_string(desc.clone()).unwrap())));
        }
        if let Some(ref emb_id) = graph_node.embedding_id {
            properties.push(("embedding_id".to_string(), Value::String(DbString::from_string(emb_id.clone()).unwrap())));
        }
        // Store timestamps as ISO strings.
        properties.push(("created_at".to_string(), Value::String(DbString::from_string(graph_node.created_at.to_rfc3339()).unwrap())));
        properties.push(("updated_at".to_string(), Value::String(DbString::from_string(graph_node.updated_at.to_rfc3339()).unwrap())));
        properties.push(("version".to_string(), Value::Int(graph_node.version as i64)));

        // Convert JSON properties to SeleneDB values.
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
        let from_id = self.uuid_to_node.get(from_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Source node not found: {}", from_uuid))?;
        let to_id = self.uuid_to_node.get(to_uuid)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("Target node not found: {}", to_uuid))?;

        let edge_id = self.graph_db.create_edge(from_id, predicate, to_id, properties.to_vec())?;
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

                // Embedding generation is deferred — just mark the node for later embedding
                if graph_node.description.is_some() {
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
                            if updated.description.is_some() {
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

                // Vector search is not available in synchronous actor context.
                // Falls back to text-based search below.


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

                // Embedding generation is deferred
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

            // ================================================================
            // Agent Infrastructure Handlers
            // ================================================================

            // ── CreateAgent ─────────────────────────────
            MemoryGraphMessage::CreateAgent { input, reply_to } => {
                info!("MemoryGraph: create_agent({})", input.name);
                // Agents use the same StoreNode logic with Agent node_type
                let mut input = input;
                input.node_type = NodeType::Agent;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetAgent ────────────────────────────────
            MemoryGraphMessage::GetAgent { id, reply_to } => {
                info!("MemoryGraph: get_agent({})", id);
                let result = self.node_meta.get(&id)
                    .filter(|n| n.node_type == NodeType::Agent)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: id.clone() }));
                let _ = reply_to.send(result);
            }

            // ── GetAgentByName ──────────────────────────
            MemoryGraphMessage::GetAgentByName { name, reply_to } => {
                info!("MemoryGraph: get_agent_by_name({})", name);
                let result = self.node_meta.values()
                    .find(|n| n.node_type == NodeType::Agent && n.name == name)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: name.clone() }));
                let _ = reply_to.send(result);
            }

            // ── GetActiveAgents ─────────────────────────
            MemoryGraphMessage::GetActiveAgents { reply_to } => {
                info!("MemoryGraph: get_active_agents");
                let agents: Vec<GraphNode> = self.node_meta.values()
                    .filter(|n| {
                        n.node_type == NodeType::Agent
                            && n.properties.get("is_active")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                let _ = reply_to.send(Ok(agents));
            }

            // ── GetAgentContext ─────────────────────────
            MemoryGraphMessage::GetAgentContext { agent_id, goal, reply_to } => {
                info!("MemoryGraph: get_agent_context({})", agent_id);
                let agent = match self.node_meta.get(&agent_id) {
                    Some(a) if a.node_type == NodeType::Agent => a.clone(),
                    _ => {
                        let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::NodeNotFound { id: agent_id.clone() })));
                        return Ok(());
                    }
                };

                // Get tools via USES_TOOL relationships
                let tools: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::UsesTool && e.from_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                    .collect();

                // Get active plan via FOLLOWS_PLAN relationships
                let plan = self.edge_meta.values()
                    .find(|e| e.edge_type == RelationshipType::FollowsPlan && e.from_id == agent_id)
                    .and_then(|e| self.node_meta.get(&e.to_id).cloned());

                // Get plan steps
                let steps: Vec<GraphNode> = if let Some(ref p) = plan {
                    self.edge_meta.values()
                        .filter(|e| e.edge_type == RelationshipType::ContainsStep && e.from_id == p.id)
                        .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                        .collect()
                } else {
                    vec![]
                };

                // Recent successful executions
                let recent_successes: Vec<GraphNode> = self.node_meta.values()
                    .filter(|n| {
                        n.node_type == NodeType::Execution
                            && n.properties.get("status")
                                .and_then(|v| v.as_str())
                                == Some("success")
                    })
                    .take(5)
                    .cloned()
                    .collect();

                // Artifacts from recent executions
                let artifacts: Vec<GraphNode> = recent_successes.iter()
                    .flat_map(|exec| {
                        self.edge_meta.values()
                            .filter(|e| e.edge_type == RelationshipType::Produced && e.from_id == exec.id)
                            .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                    })
                    .collect();

                let context = AgentContext {
                    agent,
                    tools,
                    plan,
                    steps,
                    recent_successes,
                    similar_successes: vec![],
                    similar_errors: vec![],
                    artifacts,
                    current_goal: goal,
                    metadata: HashMap::new(),
                };
                let _ = reply_to.send(Ok(context));
            }

            // ── CreateTool ──────────────────────────────
            MemoryGraphMessage::CreateTool { input, reply_to } => {
                info!("MemoryGraph: create_tool({})", input.name);
                let mut input = input;
                input.node_type = NodeType::Tool;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetTool ─────────────────────────────────
            MemoryGraphMessage::GetTool { id, reply_to } => {
                info!("MemoryGraph: get_tool({})", id);
                let result = self.node_meta.get(&id)
                    .filter(|n| n.node_type == NodeType::Tool)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: id.clone() }));
                let _ = reply_to.send(result);
            }

            // ── GetToolsForAgent ────────────────────────
            MemoryGraphMessage::GetToolsForAgent { agent_id, reply_to } => {
                info!("MemoryGraph: get_tools_for_agent({})", agent_id);
                let tools: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::UsesTool && e.from_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                    .collect();
                let _ = reply_to.send(Ok(tools));
            }

            // ── GetToolByName ───────────────────────────
            MemoryGraphMessage::GetToolByName { name, reply_to } => {
                info!("MemoryGraph: get_tool_by_name({})", name);
                let result = self.node_meta.values()
                    .find(|n| n.node_type == NodeType::Tool && n.name == name)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: name.clone() }));
                let _ = reply_to.send(result);
            }

            // ── CreatePlan ──────────────────────────────
            MemoryGraphMessage::CreatePlan { input, reply_to } => {
                info!("MemoryGraph: create_plan({})", input.name);
                let mut input = input;
                input.node_type = NodeType::Plan;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetPlan ─────────────────────────────────
            MemoryGraphMessage::GetPlan { id, reply_to } => {
                info!("MemoryGraph: get_plan({})", id);
                let result = self.node_meta.get(&id)
                    .filter(|n| n.node_type == NodeType::Plan)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: id.clone() }));
                let _ = reply_to.send(result);
            }

            // ── GetPlanSteps ────────────────────────────
            MemoryGraphMessage::GetPlanSteps { plan_id, reply_to } => {
                info!("MemoryGraph: get_plan_steps({})", plan_id);
                let steps: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ContainsStep && e.from_id == plan_id)
                    .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                    .collect();
                let _ = reply_to.send(Ok(steps));
            }

            // ── GetNextStep ─────────────────────────────
            MemoryGraphMessage::GetNextStep { plan_id, current_order, reply_to } => {
                info!("MemoryGraph: get_next_step({}, order={})", plan_id, current_order);
                let next_order = current_order + 1;
                let next_step: Option<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ContainsStep && e.from_id == plan_id)
                    .filter_map(|e| self.node_meta.get(&e.to_id))
                    .find(|step| {
                        step.properties.get("step_order")
                            .and_then(|v| v.as_u64())
                            .map(|o| o as u32 == next_order)
                            .unwrap_or(false)
                    })
                    .cloned();
                let _ = reply_to.send(Ok(next_step));
            }

            // ── StartExecution ──────────────────────────
            MemoryGraphMessage::StartExecution { input, reply_to } => {
                info!("MemoryGraph: start_execution({})", input.name);
                let mut input = input;
                input.node_type = NodeType::Execution;
                // Set initial status to "running"
                let mut props = input.properties.unwrap_or_default();
                props.insert("status".to_string(), serde_json::Value::String("running".to_string()));
                props.insert("start_time".to_string(), serde_json::Value::String(Utc::now().to_rfc3339()));
                input.properties = Some(props);
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── UpdateExecutionStatus ───────────────────
            MemoryGraphMessage::UpdateExecutionStatus { id, status, result, reply_to } => {
                info!("MemoryGraph: update_execution_status({}, {})", id, status);
                match self.node_meta.get_mut(&id) {
                    Some(node) if node.node_type == NodeType::Execution => {
                        node.properties.insert("status".to_string(), serde_json::Value::String(status.clone()));
                        if status == "success" || status == "failed" {

                            node.properties.insert("end_time".to_string(), serde_json::Value::String(Utc::now().to_rfc3339()));
                        }
                        if let Some(msg) = result {
                            node.properties.insert("status_message".to_string(), serde_json::Value::String(msg));
                        }
                        node.updated_at = Utc::now();
                        node.version += 1;
                        let _ = reply_to.send(Ok(()));
                    }
                    _ => {
                        let _ = reply_to.send(Err(anyhow::anyhow!(SchemaError::NodeNotFound { id: id.clone() })));
                    }
                }
            }

            // ── GetExecution ────────────────────────────
            MemoryGraphMessage::GetExecution { id, reply_to } => {
                info!("MemoryGraph: get_execution({})", id);
                let result = self.node_meta.get(&id)
                    .filter(|n| n.node_type == NodeType::Execution)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!(SchemaError::NodeNotFound { id: id.clone() }));
                let _ = reply_to.send(result);
            }

            // ── GetExecutionHistory ─────────────────────
            MemoryGraphMessage::GetExecutionHistory { agent_id, limit, reply_to } => {
                info!("MemoryGraph: get_execution_history({})", agent_id);
                let executions: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ExecutedBy && e.to_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.from_id))
                    .take(limit)
                    .cloned()
                    .collect();
                let _ = reply_to.send(Ok(executions));
            }

            // ── GetSuccessfulExecutions ─────────────────
            MemoryGraphMessage::GetSuccessfulExecutions { agent_id, limit, reply_to } => {
                info!("MemoryGraph: get_successful_executions({})", agent_id);
                let executions: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ExecutedBy && e.to_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.from_id))
                    .filter(|n| {
                        n.properties.get("status")
                            .and_then(|v| v.as_str())
                            == Some("success")
                    })
                    .take(limit)
                    .cloned()
                    .collect();
                let _ = reply_to.send(Ok(executions));
            }

            // ── GetFailedExecutions ─────────────────────
            MemoryGraphMessage::GetFailedExecutions { agent_id, limit, reply_to } => {
                info!("MemoryGraph: get_failed_executions({})", agent_id);
                let executions: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ExecutedBy && e.to_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.from_id))
                    .filter(|n| {
                        n.properties.get("status")
                            .and_then(|v| v.as_str())
                            == Some("failed")
                    })
                    .take(limit)
                    .cloned()
                    .collect();
                let _ = reply_to.send(Ok(executions));
            }

            // ── RecordTaskResult ────────────────────────
            MemoryGraphMessage::RecordTaskResult { input, reply_to } => {
                info!("MemoryGraph: record_task_result({})", input.name);
                let mut input = input;
                input.node_type = NodeType::TaskResult;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetTaskResults ──────────────────────────
            MemoryGraphMessage::GetTaskResults { execution_id, reply_to } => {
                info!("MemoryGraph: get_task_results({})", execution_id);
                let results: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::PartOfExecution && e.to_id == execution_id)
                    .filter_map(|e| self.node_meta.get(&e.from_id))
                    .cloned()
                    .collect();
                let _ = reply_to.send(Ok(results));
            }

            // ── RecordArtifact ──────────────────────────
            MemoryGraphMessage::RecordArtifact { input, reply_to } => {
                info!("MemoryGraph: record_artifact({})", input.name);
                let mut input = input;
                input.node_type = NodeType::Artifact;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetArtifacts ────────────────────────────
            MemoryGraphMessage::GetArtifacts { execution_id, reply_to } => {
                info!("MemoryGraph: get_artifacts({})", execution_id);
                let artifacts: Vec<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::Produced && e.from_id == execution_id)
                    .filter_map(|e| self.node_meta.get(&e.to_id).cloned())
                    .collect();
                let _ = reply_to.send(Ok(artifacts));
            }

            // ── GetLatestArtifact ───────────────────────
            MemoryGraphMessage::GetLatestArtifact { agent_id, artifact_type, reply_to } => {
                info!("MemoryGraph: get_latest_artifact({}, {})", agent_id, artifact_type);
                // Find the most recent execution for this agent, then get its artifacts
                let latest_artifact: Option<GraphNode> = self.edge_meta.values()
                    .filter(|e| e.edge_type == RelationshipType::ExecutedBy && e.to_id == agent_id)
                    .filter_map(|e| self.node_meta.get(&e.from_id))
                    .max_by_key(|exec| exec.created_at)
                    .and_then(|exec| {
                        self.edge_meta.values()
                            .filter(|e| e.edge_type == RelationshipType::Produced && e.from_id == exec.id)
                            .filter_map(|e| self.node_meta.get(&e.to_id))
                            .find(|art| {
                                art.subtype.as_deref() == Some(&artifact_type)
                            })
                            .cloned()
                    });
                let _ = reply_to.send(Ok(latest_artifact));
            }

            // ── RecordError ─────────────────────────────
            MemoryGraphMessage::RecordError { input, reply_to } => {
                info!("MemoryGraph: record_error({})", input.name);
                let mut input = input;
                input.node_type = NodeType::ErrorPattern;
                let result = self.handle_store_node(input);
                let _ = reply_to.send(result);
            }

            // ── GetErrorByFingerprint ───────────────────
            MemoryGraphMessage::GetErrorByFingerprint { fingerprint, reply_to } => {
                info!("MemoryGraph: get_error_by_fingerprint({})", fingerprint);
                let error = self.node_meta.values()
                    .find(|n| {
                        n.node_type == NodeType::ErrorPattern
                            && n.properties.get("fingerprint")
                                .and_then(|v| v.as_str())
                                == Some(&fingerprint)
                    })
                    .cloned();
                let _ = reply_to.send(Ok(error));
            }

            // ── GetSimilarErrors ────────────────────────
            MemoryGraphMessage::GetSimilarErrors { embedding_id, limit, reply_to } => {
                info!("MemoryGraph: get_similar_errors({})", embedding_id);
                // Text-based fallback: find errors with matching tags or type
                let source_node = self.node_meta.values()
                    .find(|n| n.embedding_id.as_deref() == Some(&embedding_id));
                let similar: Vec<GraphNode> = match source_node {
                    Some(src) => {
                        let src_tags: Vec<String> = src.properties.get("tags")
                            .and_then(|v| v.as_array())
                            .map(|arr| {
                                arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                            })
                            .unwrap_or_default();
                        self.node_meta.values()
                            .filter(|n| {
                                n.node_type == NodeType::ErrorPattern && n.id != src.id
                            })
                            .filter(|n| {
                                // Match if any tag overlaps
                                let tags: Vec<String> = n.properties.get("tags")
                                    .and_then(|v| v.as_array())
                                    .map(|arr| {
                                        arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
                                    })
                                    .unwrap_or_default();
                                tags.iter().any(|t| src_tags.contains(t))
                            })
                            .take(limit)
                            .cloned()
                            .collect()
                    }
                    None => vec![],
                };
                let _ = reply_to.send(Ok(similar));
            }

            // ── LinkErrorToFix ──────────────────────────
            MemoryGraphMessage::LinkErrorToFix { error_id, execution_id, properties, reply_to } => {
                info!("MemoryGraph: link_error_to_fix({} -> {})", error_id, execution_id);
                // Create a RESOLVED_BY relationship from error to execution
                let rel = RelationshipInput {
                    edge_type: RelationshipType::ResolvedBy,
                    from_id: error_id,
                    to_id: execution_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result.map(|_| ()));
            }

            // ── CreateUsesTool ──────────────────────────
            MemoryGraphMessage::CreateUsesTool { agent_id, tool_id, properties, reply_to } => {
                info!("MemoryGraph: create_uses_tool({} -> {})", agent_id, tool_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::UsesTool,
                    from_id: agent_id,
                    to_id: tool_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateFollowsPlan ───────────────────────
            MemoryGraphMessage::CreateFollowsPlan { agent_id, plan_id, properties, reply_to } => {
                info!("MemoryGraph: create_follows_plan({} -> {})", agent_id, plan_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::FollowsPlan,
                    from_id: agent_id,
                    to_id: plan_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateContainsStep ──────────────────────
            MemoryGraphMessage::CreateContainsStep { plan_id, step_id, properties, reply_to } => {
                info!("MemoryGraph: create_contains_step({} -> {})", plan_id, step_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::ContainsStep,
                    from_id: plan_id,
                    to_id: step_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreatePrecedes ──────────────────────────
            MemoryGraphMessage::CreatePrecedes { from_step_id, to_step_id, properties, reply_to } => {
                info!("MemoryGraph: create_precedes({} -> {})", from_step_id, to_step_id);
                // Enforce acyclic constraint for precedes relationships
                if self.would_create_precedes_cycle(&from_step_id, &to_step_id) {
                    let _ = reply_to.send(Err(anyhow::anyhow!(
                        SchemaError::AcyclicPrecedesViolation {
                            from: from_step_id.clone(),
                            to: to_step_id.clone(),
                        }
                    )));
                    return Ok(());
                }
                let rel = RelationshipInput {
                    edge_type: RelationshipType::Precedes,
                    from_id: from_step_id,
                    to_id: to_step_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateProduced ──────────────────────────
            MemoryGraphMessage::CreateProduced { execution_id, artifact_id, properties, reply_to } => {
                info!("MemoryGraph: create_produced({} -> {})", execution_id, artifact_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::Produced,
                    from_id: execution_id,
                    to_id: artifact_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateEncounteredError ──────────────────
            MemoryGraphMessage::CreateEncounteredError { execution_id, error_id, properties, reply_to } => {
                info!("MemoryGraph: create_encountered_error({} -> {})", execution_id, error_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::EncounteredError,
                    from_id: execution_id,
                    to_id: error_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateResolvedBy ────────────────────────
            MemoryGraphMessage::CreateResolvedBy { error_id, execution_id, properties, reply_to } => {
                info!("MemoryGraph: create_resolved_by({} -> {})", error_id, execution_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::ResolvedBy,
                    from_id: error_id,
                    to_id: execution_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreatePartOfExecution ───────────────────
            MemoryGraphMessage::CreatePartOfExecution { task_result_id, execution_id, properties, reply_to } => {
                info!("MemoryGraph: create_part_of_execution({} -> {})", task_result_id, execution_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::PartOfExecution,
                    from_id: task_result_id,
                    to_id: execution_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateExecutedBy ────────────────────────
            MemoryGraphMessage::CreateExecutedBy { execution_id, agent_id, properties, reply_to } => {
                info!("MemoryGraph: create_executed_by({} -> {})", execution_id, agent_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::ExecutedBy,
                    from_id: execution_id,
                    to_id: agent_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
            }

            // ── CreateLearnedFrom ───────────────────────
            MemoryGraphMessage::CreateLearnedFrom { agent_id, execution_id, properties, reply_to } => {
                info!("MemoryGraph: create_learned_from({} -> {})", agent_id, execution_id);
                let rel = RelationshipInput {
                    edge_type: RelationshipType::LearnedFrom,
                    from_id: agent_id,
                    to_id: execution_id,
                    properties: Some(properties),
                    weight: None,
                };
                let result = self.handle_create_relationship(rel);
                let _ = reply_to.send(result);
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
        serde_json::Value::String(s) => Some(Value::String(DbString::from_string(s.clone()).unwrap())),
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
                        Value::List(vec![Value::String(DbString::from_string(k).unwrap()), v])
                    })
                    .collect(),
            ))
        }
    }
}
