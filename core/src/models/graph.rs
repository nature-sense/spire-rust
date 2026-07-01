use serde::{Deserialize, Serialize};

use selene_core::identity::{EdgeId, NodeId};
use selene_core::value::Value;

use crate::models::memory_graph::{GraphEdge, GraphNode};

/// A node in the SeleneDB-backed graph, using SeleneDB's native types.
///
/// This is the low-level representation used by `GraphDb` for storage.
/// The higher-level `GraphNode` (from `models::memory_graph`) is used
/// for the external API.
#[derive(Debug, Clone)]
pub struct Node {
    /// The SeleneDB node ID. Use `NodeId::TOMBSTONE` for new nodes.
    pub id: NodeId,
    /// Labels for this node (e.g., `["Person", "Developer"]`).
    pub labels: Vec<String>,
    /// Key-value properties.
    pub properties: Vec<(String, Value)>,
}

/// An edge in the SeleneDB-backed graph, using SeleneDB's native types.
///
/// This is the low-level representation used by `GraphDb` for storage.
#[derive(Debug, Clone)]
pub struct Edge {
    /// The SeleneDB edge ID.
    pub id: EdgeId,
    /// The source node ID.
    pub subject: NodeId,
    /// The predicate/label of the edge.
    pub predicate: String,
    /// The target node ID.
    pub object: NodeId,
    /// Key-value properties.
    pub properties: Vec<(String, Value)>,
}

/// A query against the knowledge graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphQuery {
    pub query_type: GraphQueryType,
    pub node_id: Option<String>,
    pub node_label: Option<String>,
    pub max_depth: Option<usize>,
    pub limit: Option<usize>,
}

/// The type of graph query to perform.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphQueryType {
    /// Find neighbors of a node
    Neighbors,
    /// Find paths between two nodes
    Path,
    /// Search nodes by label
    Search,
    /// Get subgraph around a node
    Subgraph,
}

/// The result of a graph query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphResult {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub total_count: usize,
}

