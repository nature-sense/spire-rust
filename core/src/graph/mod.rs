//! Graph database wrapper around SeleneDB.
//!
//! This module provides a high-level wrapper around SeleneDB's graph database,
//! exposing a simplified API for node/edge CRUD, traversal, and vector search.
//! Persistence is handled via Write-Ahead Log (WAL).

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use selene_core::identity::{EdgeId, GraphId, NodeId};
use selene_core::label_set::LabelSet;
use selene_core::property_map::PropertyMap;
use selene_core::value::Value;
use selene_core::vector::VectorMetric;
use selene_graph::graph::SeleneGraph;
use selene_graph::shared::SharedGraph;
use selene_graph::write_txn::WriteTxn;
use selene_graph::mutator::Mutator;
use selene_graph::vector_index::VectorIndexConfig;
use selene_graph::vector_search::VectorSearchHit;
use selene_graph::GraphResult;
use selene_persist::WalConfig;

use crate::models::memory_graph::{GraphEdge, GraphNode};

/// A high-level wrapper around SeleneDB's graph database.
///
/// `GraphDb` manages a `SharedGraph` instance with optional WAL-based persistence.
/// It provides a simplified API for common graph operations used by the Spire
/// knowledge graph.
pub struct GraphDb {
    /// The underlying SeleneDB shared graph.
    shared: Arc<SharedGraph>,
    /// The graph ID used for this database instance.
    graph_id: GraphId,
}

impl GraphDb {
    /// Create a new in-memory graph database (no persistence).
    pub fn new_in_memory() -> Result<Self> {
        let graph_id = GraphId::generate();
        let graph = SeleneGraph::new(graph_id);
        let shared = SharedGraph::from_graph(graph)
            .map_err(|e| anyhow::anyhow!("Failed to create in-memory graph: {}", e))?;

        Ok(Self {
            shared: Arc::new(shared),
            graph_id,
        })
    }

    /// Create a new graph database with WAL-based persistence.
    ///
    /// The WAL file will be created at `wal_path`. If a WAL already exists at
    /// that path, it will be recovered on open.
    pub fn new_with_wal(wal_path: impl AsRef<Path>) -> Result<Self> {
        let graph_id = GraphId::generate();
        let graph = SeleneGraph::new(graph_id);
        let config = WalConfig::default();
        let shared = SharedGraph::from_graph_with_wal(graph, wal_path.as_ref(), config)
            .map_err(|e| anyhow::anyhow!("Failed to create WAL-backed graph: {}", e))?;

        Ok(Self {
            shared: Arc::new(shared),
            graph_id,
        })
    }

    /// Get the graph ID.
    pub fn graph_id(&self) -> GraphId {
        self.graph_id
    }

    /// Get the number of live nodes in the graph.
    pub fn node_count(&self) -> usize {
        let snapshot = self.shared.read();
        snapshot.node_count()
    }

    /// Get the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        let snapshot = self.shared.read();
        snapshot.edge_count()
    }

    // ─── Node Operations ───────────────────────────────────────────────

    /// Create a new node with the given labels and properties.
    ///
    /// Returns the newly assigned `NodeId`.
    pub fn create_node(&self, labels: Vec<String>, properties: Vec<(String, Value)>) -> Result<NodeId> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        let label_set = LabelSet::from(labels);
        let mut prop_map = PropertyMap::new();
        for (key, value) in properties {
            prop_map.insert(key.into(), value);
        }

        let node_id = mutator
            .create_node(label_set, prop_map)
            .map_err(|e| anyhow::anyhow!("Failed to create node: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit node creation: {}", e))?;

        Ok(node_id)
    }

    /// Get a node by its ID.
    pub fn get_node(&self, node_id: NodeId) -> Option<GraphNode> {
        let snapshot = self.shared.read();
        let labels = snapshot.node_labels(node_id)?;
        let properties = snapshot.node_properties(node_id)?;

        Some(GraphNode {
            id: node_id.to_string(),
            labels: labels.iter().map(|l| l.to_string()).collect(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        })
    }

    /// Delete a node by its ID.
    pub fn delete_node(&self, node_id: NodeId) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        // Use update_node with empty diff to mark as deleted (tombstone)
        // SeleneDB uses tombstones for deletion
        mutator
            .update_node(node_id, selene_graph::graph_types::PropertyDiff::default())
            .map_err(|e| anyhow::anyhow!("Failed to delete node: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit node deletion: {}", e))?;

        Ok(())
    }

    // ─── Edge Operations ───────────────────────────────────────────────

    /// Create a new edge between two nodes.
    ///
    /// * `subject` - Source node ID
    /// * `predicate` - Edge label/type
    /// * `object` - Target node ID
    /// * `properties` - Edge properties
    pub fn create_edge(
        &self,
        subject: NodeId,
        predicate: &str,
        object: NodeId,
        properties: Vec<(String, Value)>,
    ) -> Result<EdgeId> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        let mut prop_map = PropertyMap::new();
        for (key, value) in properties {
            prop_map.insert(key.into(), value);
        }

        let edge_id = mutator
            .create_edge(subject, predicate.into(), object, prop_map)
            .map_err(|e| anyhow::anyhow!("Failed to create edge: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit edge creation: {}", e))?;

        Ok(edge_id)
    }

    /// Get an edge by its ID.
    pub fn get_edge(&self, edge_id: EdgeId) -> Option<GraphEdge> {
        let snapshot = self.shared.read();
        let label = snapshot.edge_label(edge_id)?;
        let endpoints = snapshot.edge_endpoints(edge_id)?;
        let properties = snapshot.edge_properties(edge_id)?;

        Some(GraphEdge {
            id: edge_id.to_string(),
            subject: endpoints.0.to_string(),
            predicate: label.to_string(),
            object: endpoints.1.to_string(),
            properties: properties
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
        })
    }

    /// Delete an edge by its ID.
    pub fn delete_edge(&self, edge_id: EdgeId) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        mutator
            .update_edge(edge_id, selene_graph::graph_types::PropertyDiff::default())
            .map_err(|e| anyhow::anyhow!("Failed to delete edge: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit edge deletion: {}", e))?;

        Ok(())
    }

    // ─── Traversal Operations ──────────────────────────────────────────

    /// Get outgoing edges from a node.
    pub fn outgoing_edges(&self, node_id: NodeId) -> Vec<GraphEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        if let Some(adj) = snapshot.outgoing_edges(node_id) {
            for edge_ref in adj.iter() {
                let edge_id = edge_ref.edge_id();
                if let Some(label) = snapshot.edge_label(edge_id) {
                    if let Some(endpoints) = snapshot.edge_endpoints(edge_id) {
                        let properties = snapshot.edge_properties(edge_id).cloned().unwrap_or_default();
                        edges.push(GraphEdge {
                            id: edge_id.to_string(),
                            subject: endpoints.0.to_string(),
                            predicate: label.to_string(),
                            object: endpoints.1.to_string(),
                            properties: properties
                                .iter()
                                .map(|(k, v)| (k.to_string(), v.clone()))
                                .collect(),
                        });
                    }
                }
            }
        }

        edges
    }

    /// Get incoming edges to a node.
    pub fn incoming_edges(&self, node_id: NodeId) -> Vec<GraphEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        if let Some(adj) = snapshot.incoming_edges(node_id) {
            for edge_ref in adj.iter() {
                let edge_id = edge_ref.edge_id();
                if let Some(label) = snapshot.edge_label(edge_id) {
                    if let Some(endpoints) = snapshot.edge_endpoints(edge_id) {
                        let properties = snapshot.edge_properties(edge_id).cloned().unwrap_or_default();
                        edges.push(GraphEdge {
                            id: edge_id.to_string(),
                            subject: endpoints.0.to_string(),
                            predicate: label.to_string(),
                            object: endpoints.1.to_string(),
                            properties: properties
                                .iter()
                                .map(|(k, v)| (k.to_string(), v.clone()))
                                .collect(),
                        });
                    }
                }
            }
        }

        edges
    }

    /// Find nodes by label.
    pub fn nodes_with_label(&self, label: &str) -> Vec<GraphNode> {
        let snapshot = self.shared.read();
        let mut nodes = Vec::new();

        if let Some(bitmap) = snapshot.nodes_with_label(&label.into()) {
            for node_id in bitmap.iter() {
                if let Some(node) = self.get_node(NodeId::from(node_id)) {
                    nodes.push(node);
                }
            }
        }

        nodes
    }

    /// Find edges by label.
    pub fn edges_with_label(&self, label: &str) -> Vec<GraphEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        if let Some(bitmap) = snapshot.edges_with_label(&label.into()) {
            for edge_id in bitmap.iter() {
                if let Some(edge) = self.get_edge(EdgeId::from(edge_id)) {
                    edges.push(edge);
                }
            }
        }

        edges
    }

    // ─── Vector Index Operations ───────────────────────────────────────

    /// Create a vector index for a specific label and property.
    ///
    /// This enables semantic search over nodes with the given label,
    /// using the specified property as the embedding source.
    pub fn create_vector_index(
        &self,
        label: &str,
        property: &str,
        dimensions: usize,
        metric: VectorMetric,
    ) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        let config = VectorIndexConfig {
            dimensions,
            metric,
            ..Default::default()
        };

        mutator
            .schema_change(
                self.graph_id,
                selene_graph::graph_types::SchemaChange::AddVectorIndex {
                    label: label.into(),
                    property: property.into(),
                    config,
                },
            )
            .map_err(|e| anyhow::anyhow!("Failed to create vector index: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit vector index creation: {}", e))?;

        Ok(())
    }

    /// Perform a vector similarity search.
    ///
    /// Searches for nodes with the given label whose embedding property
    /// is closest to the query vector.
    pub fn vector_search(
        &self,
        label: &str,
        property: &str,
        query_vector: Vec<f32>,
        limit: usize,
    ) -> Result<Vec<VectorSearchHit>> {
        let snapshot = self.shared.read();

        let index = snapshot
            .vector_index_for(&label.into(), &property.into())
            .ok_or_else(|| anyhow::anyhow!("Vector index not found for {}.{}", label, property))?;

        let hits = index
            .search(&query_vector, limit)
            .map_err(|e| anyhow::anyhow!("Vector search failed: {}", e))?;

        Ok(hits)
    }

    // ─── Maintenance ───────────────────────────────────────────────────

    /// Compact the graph database, reclaiming space from tombstones.
    pub fn compact(&self) -> Result<()> {
        self.shared
            .compact()
            .map_err(|e| anyhow::anyhow!("Compaction failed: {}", e))?;
        Ok(())
    }

    /// Rebuild all vector indexes.
    pub fn rebuild_vector_indexes(&self) -> Result<()> {
        self.shared
            .rebuild_vector_indexes()
            .map_err(|e| anyhow::anyhow!("Vector index rebuild failed: {}", e))?;
        Ok(())
    }
}

impl Clone for GraphDb {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            graph_id: self.graph_id,
        }
    }
}

unsafe impl Send for GraphDb {}
unsafe impl Sync for GraphDb {}
