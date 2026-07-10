// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Graph database wrapper around SeleneDB.
//!
//! This module provides a high-level wrapper around SeleneDB's graph database,
//! exposing a simplified API for node/edge CRUD, traversal, and vector search.
//! Persistence is handled via Write-Ahead Log (WAL) and periodic snapshots.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use selene_db_core::db_string::DbString;
use selene_db_core::identity::{EdgeId, GraphId, NodeId};
use selene_db_core::label_set::LabelSet;
use selene_db_core::property_map::PropertyMap;
use selene_db_core::value::{Value, VectorValue};
use selene_db_core::vector::VectorMetric;
use selene_db_graph::shared::SharedGraph;
use selene_db_graph::store::RowIndex;
use selene_db_graph::vector_index::VectorIndexKind;
use selene_db_graph::vector_search::VectorNodeSearchHit;
use selene_db_persist::{
    SectionCompression, SnapshotConfig, SnapshotFinalizeOutcome, WalConfig, find_latest_snapshot,
    snapshot_path,
};

/// A node in the graph database (low-level representation).
#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub labels: Vec<String>,
    pub properties: HashMap<String, Value>,
}

/// An edge in the graph database (low-level representation).
#[derive(Debug, Clone)]
pub struct GraphEdge {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub properties: HashMap<String, Value>,
}

/// Helper to convert a string to DbString (using TryFrom).
fn to_db_string(s: &str) -> DbString {
    DbString::try_from(s).expect("valid DbString")
}

/// Helper to convert a String to DbString (using from_string).
fn to_db_string_owned(s: String) -> DbString {
    DbString::from_string(s).expect("valid DbString")
}

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
        let graph_id = GraphId::new(1);
        let shared = SharedGraph::new(graph_id);

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
        let graph_id = GraphId::new(1);
        let config = WalConfig::default();
        let shared = SharedGraph::builder(graph_id)
            .with_wal(wal_path.as_ref(), config)
            .map_err(|e| anyhow::anyhow!("Failed to create WAL-backed graph: {}", e))?;

        Ok(Self {
            shared: Arc::new(shared.build().map_err(|e| anyhow::anyhow!("Failed to build shared graph: {}", e))?),
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

        let label_set = LabelSet::from_iter(labels.into_iter().map(|s| to_db_string_owned(s)));
        let mut prop_map = PropertyMap::new();
        for (key, value) in properties {
            prop_map.set(to_db_string_owned(key), value).map_err(|e| anyhow::anyhow!("Failed to set property: {}", e))?;
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

        mutator
            .delete_node(node_id)
            .map_err(|e| anyhow::anyhow!("Failed to delete node: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit node deletion: {}", e))?;

        Ok(())
    }

    // ─── Edge Operations ───────────────────────────────────────────────

    /// Create a new edge between two nodes.
    ///
    /// * `label` - Edge label/type (predicate)
    /// * `subject` - Source node ID
    /// * `object` - Target node ID
    /// * `properties` - Edge properties
    pub fn create_edge(
        &self,
        label: &str,
        subject: NodeId,
        object: NodeId,
        properties: Vec<(String, Value)>,
    ) -> Result<EdgeId> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        let mut prop_map = PropertyMap::new();
        for (key, value) in properties {
            prop_map.set(to_db_string_owned(key), value).map_err(|e| anyhow::anyhow!("Failed to set property: {}", e))?;
        }

        let edge_id = mutator
            .create_edge(to_db_string(label), subject, object, prop_map)
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
            .delete_edge(edge_id)
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
                let edge_id = edge_ref.edge_id;
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
                let edge_id = edge_ref.edge_id;
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
        let db_label = to_db_string(label);

        if let Some(bitmap) = snapshot.nodes_with_label(&db_label) {
            for row in bitmap.iter() {
                if let Some(node_id) = snapshot.node_id_for_row(RowIndex::new(row)) {
                    if let Some(node) = self.get_node(node_id) {
                        nodes.push(node);
                    }
                }
            }
        }

        nodes
    }

    /// Find edges by label.
    pub fn edges_with_label(&self, label: &str) -> Vec<GraphEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();
        let db_label = to_db_string(label);

        if let Some(bitmap) = snapshot.edges_with_label(&db_label) {
            for row in bitmap.iter() {
                if let Some(edge_id) = snapshot.edge_id_for_row(RowIndex::new(row)) {
                    if let Some(edge) = self.get_edge(edge_id) {
                        edges.push(edge);
                    }
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
        dimensions: u32,
        kind: VectorIndexKind,
    ) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        mutator
            .create_vector_index(
                to_db_string(label),
                to_db_string(property),
                kind,
                dimensions,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create vector index: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit vector index creation: {}", e))?;

        Ok(())
    }

    /// Perform an exact vector similarity search.
    ///
    /// Searches for nodes with the given label whose embedding property
    /// is closest to the query vector, using exhaustive scan.
    pub fn exact_vector_search(
        &self,
        label: &str,
        property: &str,
        query_vector: Vec<f32>,
        metric: VectorMetric,
        limit: usize,
    ) -> Result<Vec<VectorNodeSearchHit>> {
        let snapshot = self.shared.read();
        let query = VectorValue::try_from(query_vector)
            .map_err(|e| anyhow::anyhow!("Failed to create vector value: {}", e))?;

        let hits = snapshot
            .exact_vector_search_nodes(
                &to_db_string(label),
                &to_db_string(property),
                &query,
                metric,
                limit,
            )
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

    /// Clear all data from the graph by iterating live nodes/edges and deleting them.
    ///
    /// This is useful for testing or resetting the database to a clean state.
    /// All data is permanently lost.
    pub fn clear(&self) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        // Delete all edges first (to avoid orphan issues)
        let snapshot = self.shared.read();
        let live_edges = snapshot.live_edges().clone();
        drop(snapshot);

        for row in live_edges.iter() {
            let snapshot = self.shared.read();
            if let Some(edge_id) = snapshot.edge_id_for_row(RowIndex::new(row)) {
                drop(snapshot);
                let _ = mutator.delete_edge(edge_id);
            } else {
                drop(snapshot);
            }
        }

        // Delete all nodes
        let snapshot = self.shared.read();
        let live_nodes = snapshot.live_nodes().clone();
        drop(snapshot);

        for row in live_nodes.iter() {
            let snapshot = self.shared.read();
            if let Some(node_id) = snapshot.node_id_for_row(RowIndex::new(row)) {
                drop(snapshot);
                let _ = mutator.delete_node(node_id);
            } else {
                drop(snapshot);
            }
        }

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit clear: {}", e))?;

        Ok(())
    }

    /// Get a reference to the underlying `SharedGraph`.
    ///
    /// This is used by the GQL session layer to execute GQL statements
    /// directly against the graph.
    pub fn shared_graph(&self) -> &Arc<SharedGraph> {
        &self.shared
    }

    // ─── Persistence (Snapshots + Recovery) ────────────────────────────

    /// Recover a graph database from a persistence directory.
    ///
    /// This loads the most recent snapshot (if any), replays WAL entries
    /// written after that snapshot, and returns a live WAL-backed graph
    /// ready for reads and writes.
    ///
    /// If no snapshot exists and the WAL is empty, this returns an empty
    /// graph (equivalent to `new_in_memory` but with WAL attached).
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory containing snapshot and WAL files.
    /// * `graph_id` - The graph ID to use (must match the snapshot if one exists).
    pub fn recover(dir: impl AsRef<Path>, graph_id: GraphId) -> Result<Self> {
        let shared = SharedGraph::recover(dir.as_ref(), graph_id)
            .map_err(|e| anyhow::anyhow!("Failed to recover graph: {}", e))?;

        Ok(Self {
            shared: Arc::new(shared),
            graph_id,
        })
    }

    /// Write a point-in-time snapshot of the current graph state to disk.
    ///
    /// Snapshots capture the full graph state (nodes, edges, schemas, indexes)
    /// and are used together with the WAL for crash recovery. Call this
    /// periodically to bound recovery time.
    ///
    /// # Arguments
    ///
    /// * `dir` - Directory where the snapshot file will be written.
    /// * `sequence` - Monotonically increasing snapshot sequence number.
    ///   Must be higher than any previously written snapshot sequence.
    /// * `fsync` - Whether to fsync the snapshot file before finalizing.
    ///
    /// Returns the snapshot outcome including the sequence number and
    /// section count.
    pub fn write_snapshot(
        &self,
        dir: impl AsRef<Path>,
        sequence: u64,
        fsync: bool,
    ) -> Result<SnapshotFinalizeOutcome> {
        let config = SnapshotConfig {
            dir: dir.as_ref().to_path_buf(),
            sequence,
            compression: SectionCompression::default(),
            fsync,
        };
        self.shared
            .write_snapshot(config)
            .map_err(|e| anyhow::anyhow!("Failed to write snapshot: {}", e))
    }

    /// Find the latest snapshot sequence number in a directory.
    ///
    /// Returns `None` if no snapshot exists.
    pub fn latest_snapshot_sequence(dir: impl AsRef<Path>) -> Result<Option<u64>> {
        match find_latest_snapshot(dir.as_ref()) {
            Ok(Some((seq, _path))) => Ok(Some(seq)),
            Ok(None) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Failed to find latest snapshot: {}", e)),
        }
    }

    /// Get the path for a snapshot file at a given sequence number.
    pub fn snapshot_path(dir: impl AsRef<Path>, sequence: u64) -> PathBuf {
        snapshot_path(dir.as_ref(), sequence)
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

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use selene_db_core::value::Value;
    use selene_db_graph::vector_index::VectorIndexKind;

    fn create_test_graph() -> GraphDb {
        GraphDb::new_in_memory().expect("Failed to create in-memory graph")
    }

    // ─── Node CRUD ───────────────────────────────────────────────────────

    #[test]
    fn test_create_and_get_node() {
        let db = create_test_graph();

        let node_id = db
            .create_node(
                vec!["Person".to_string(), "Developer".to_string()],
                vec![
                    ("name".to_string(), Value::String(to_db_string("Alice"))),
                    ("age".to_string(), Value::Int(30)),
                ],
            )
            .expect("Failed to create node");

        let node = db.get_node(node_id).expect("Node should exist");
        assert!(node.labels.contains(&"Person".to_string()));
        assert!(node.labels.contains(&"Developer".to_string()));
        assert_eq!(node.properties.get("name").and_then(|v| {
            if let Value::String(s) = v {
                Some(s.to_string())
            } else {
                None
            }
        }), Some("Alice".to_string()));
    }

    #[test]
    fn test_delete_node() {
        let db = create_test_graph();

        let node_id = db
            .create_node(vec!["Temp".to_string()], vec![("x".to_string(), Value::Int(1))])
            .expect("Failed to create node");

        assert!(db.get_node(node_id).is_some());
        db.delete_node(node_id).expect("Failed to delete node");
        assert!(db.get_node(node_id).is_none());
    }

    #[test]
    fn test_delete_nonexistent_node_returns_error() {
        let db = create_test_graph();
        let result = db.delete_node(NodeId::new(99999));
        assert!(result.is_err());
    }

    #[test]
    fn test_node_count() {
        let db = create_test_graph();
        assert_eq!(db.node_count(), 0);

        let n1 = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let n2 = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        assert_eq!(db.node_count(), 2);

        db.delete_node(n1).unwrap();
        assert_eq!(db.node_count(), 1);

        db.delete_node(n2).unwrap();
        assert_eq!(db.node_count(), 0);
    }

    // ─── Edge CRUD ───────────────────────────────────────────────────────

    #[test]
    fn test_create_and_get_edge() {
        let db = create_test_graph();

        let alice = db
            .create_node(vec!["Person".to_string()], vec![("name".to_string(), Value::String(to_db_string("Alice")))])
            .unwrap();
        let bob = db
            .create_node(vec!["Person".to_string()], vec![("name".to_string(), Value::String(to_db_string("Bob")))])
            .unwrap();

        let edge_id = db
            .create_edge("knows", alice, bob, vec![("since".to_string(), Value::Int(2020))])
            .expect("Failed to create edge");

        let edge = db.get_edge(edge_id).expect("Edge should exist");
        assert_eq!(edge.predicate, "knows");
        assert_eq!(edge.subject, alice.to_string());
        assert_eq!(edge.object, bob.to_string());
    }

    #[test]
    fn test_delete_edge() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        let e = db.create_edge("edge", a, b, vec![]).unwrap();

        assert!(db.get_edge(e).is_some());
        db.delete_edge(e).expect("Failed to delete edge");
        assert!(db.get_edge(e).is_none());
    }

    #[test]
    fn test_edge_count() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        let c = db.create_node(vec!["C".to_string()], vec![]).unwrap();

        assert_eq!(db.edge_count(), 0);

        db.create_edge("e1", a, b, vec![]).unwrap();
        assert_eq!(db.edge_count(), 1);

        db.create_edge("e2", b, c, vec![]).unwrap();
        assert_eq!(db.edge_count(), 2);
    }

    // ─── Label Queries ───────────────────────────────────────────────────

    #[test]
    fn test_nodes_with_label() {
        let db = create_test_graph();

        let _n1 = db.create_node(vec!["Person".to_string()], vec![("name".to_string(), Value::String(to_db_string("Alice")))]).unwrap();
        let _n2 = db.create_node(vec!["Person".to_string()], vec![("name".to_string(), Value::String(to_db_string("Bob")))]).unwrap();
        let _n3 = db.create_node(vec!["Animal".to_string()], vec![("name".to_string(), Value::String(to_db_string("Charlie")))]).unwrap();

        let persons = db.nodes_with_label("Person");
        assert_eq!(persons.len(), 2);

        let animals = db.nodes_with_label("Animal");
        assert_eq!(animals.len(), 1);

        let nonexistent = db.nodes_with_label("Nonexistent");
        assert!(nonexistent.is_empty());
    }

    #[test]
    fn test_edges_with_label() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        let c = db.create_node(vec!["C".to_string()], vec![]).unwrap();

        db.create_edge("knows", a, b, vec![]).unwrap();
        db.create_edge("knows", b, c, vec![]).unwrap();
        db.create_edge("likes", a, c, vec![]).unwrap();

        let knows = db.edges_with_label("knows");
        assert_eq!(knows.len(), 2);

        let likes = db.edges_with_label("likes");
        assert_eq!(likes.len(), 1);
    }

    // ─── Directional Edges ───────────────────────────────────────────────

    #[test]
    fn test_outgoing_and_incoming_edges() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();

        db.create_edge("knows", a, b, vec![]).unwrap();

        let outgoing = db.outgoing_edges(a);
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].object, b.to_string());

        let incoming = db.incoming_edges(b);
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].subject, a.to_string());

        // A has no incoming, B has no outgoing
        assert!(db.incoming_edges(a).is_empty());
        assert!(db.outgoing_edges(b).is_empty());
    }

    // ─── Vector Index ────────────────────────────────────────────────────

    #[test]
    fn test_vector_search() {
        let db = create_test_graph();

        // Create vector index
        db.create_vector_index("Item", "embedding", 4, VectorIndexKind::Flat)
            .expect("Failed to create vector index");

        // Create nodes with embedding vectors
        let _n1 = db
            .create_node(
                vec!["Item".to_string()],
                vec![
                    ("name".to_string(), Value::String(to_db_string("A"))),
                    (
                        "embedding".to_string(),
                        Value::Vector(
                            VectorValue::try_from(vec![1.0f32, 0.0, 0.0, 0.0]).unwrap(),
                        ),
                    ),
                ],
            )
            .unwrap();

        let _n2 = db
            .create_node(
                vec!["Item".to_string()],
                vec![
                    ("name".to_string(), Value::String(to_db_string("B"))),
                    (
                        "embedding".to_string(),
                        Value::Vector(
                            VectorValue::try_from(vec![0.0f32, 1.0, 0.0, 0.0]).unwrap(),
                        ),
                    ),
                ],
            )
            .unwrap();

        // Rebuild vector indexes so newly added vectors are indexed
        db.rebuild_vector_indexes().expect("Failed to rebuild indexes");

        // Search for something close to [1.0, 0.0, 0.0, 0.0]
        let hits = db
            .exact_vector_search(
                "Item",
                "embedding",
                vec![0.9f32, 0.1, 0.0, 0.0],
                VectorMetric::Cosine,
                5,
            )
            .expect("Vector search failed");

        assert!(!hits.is_empty(), "Expected at least one hit");
        // The first hit should be node A (closest to query)
        // Cosine distance = 1 - cosine_similarity, so near-identical vectors have distance near 0
        assert!(hits[0].distance < 0.1, "Expected small cosine distance, got {}", hits[0].distance);
    }

    // ─── Clear ───────────────────────────────────────────────────────────

    #[test]
    fn test_clear_graph() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        db.create_edge("e", a, b, vec![]).unwrap();

        assert_eq!(db.node_count(), 2);
        assert_eq!(db.edge_count(), 1);

        db.clear().expect("Failed to clear graph");

        assert_eq!(db.node_count(), 0);
        assert_eq!(db.edge_count(), 0);
    }

    // ─── WAL Persistence ─────────────────────────────────────────────────
    //
    // SeleneDB's WAL (Write-Ahead Log) provides crash recovery for the
    // committer thread — it ensures that committed mutations survive a crash
    // before they are published. The WAL is an append-only log; the in-memory
    // graph state is rebuilt from a snapshot, not by replaying the WAL.
    //
    // The tests below verify that:
    // 1. A WAL-backed graph can be created and used.
    // 2. The WAL file is created on disk.
    // 3. A new graph can be opened with the same WAL path (the WAL writer
    //    opens existing files with truncate(false) and scans for valid entries).

    #[test]
    fn test_wal_file_created() {
        let dir = std::env::temp_dir().join(format!("spire_test_wal_file_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let wal_path = dir.join("test.wal");

        // Create graph with WAL and write data
        {
            let db = GraphDb::new_with_wal(&wal_path).expect("Failed to create WAL graph");
            let n1 = db.create_node(vec!["Persistent".to_string()], vec![("key".to_string(), Value::String(to_db_string("value1")))]).unwrap();
            let n2 = db.create_node(vec!["Persistent".to_string()], vec![("key".to_string(), Value::String(to_db_string("value2")))]).unwrap();
            db.create_edge("link", n1, n2, vec![]).unwrap();
            assert_eq!(db.node_count(), 2);
            assert_eq!(db.edge_count(), 1);
            // Drop goes out of scope — WAL is flushed
        }

        // Verify WAL file exists and has content
        assert!(wal_path.exists(), "WAL file should exist on disk");
        let wal_len = std::fs::metadata(&wal_path).expect("Failed to read WAL metadata").len();
        assert!(wal_len > 0, "WAL file should have content (got {} bytes)", wal_len);

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_wal_reopen_same_path() {
        let dir = std::env::temp_dir().join(format!("spire_test_wal_reopen_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let wal_path = dir.join("test.wal");

        // Phase 1: create graph, write data
        {
            let db = GraphDb::new_with_wal(&wal_path).expect("Failed to create WAL graph");
            let n1 = db.create_node(vec!["Persistent".to_string()], vec![("key".to_string(), Value::String(to_db_string("value1")))]).unwrap();
            let n2 = db.create_node(vec!["Persistent".to_string()], vec![("key".to_string(), Value::String(to_db_string("value2")))]).unwrap();
            db.create_edge("link", n1, n2, vec![]).unwrap();
            assert_eq!(db.node_count(), 2);
            assert_eq!(db.edge_count(), 1);
        }

        // Phase 2: open a new graph with the same WAL path.
        // The WAL writer opens existing files with truncate(false) and
        // scans for valid entries. The graph itself starts fresh (the WAL
        // is for crash recovery of the committer, not for replay).
        {
            let db = GraphDb::new_with_wal(&wal_path).expect("Failed to reopen WAL graph");
            // The new graph starts empty — WAL entries are not replayed
            // into the in-memory graph state.
            assert_eq!(db.node_count(), 0, "New graph starts empty; WAL is for crash recovery, not replay");
            assert_eq!(db.edge_count(), 0);
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_wal_empty_reopen() {
        let dir = std::env::temp_dir().join(format!("spire_test_wal_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let wal_path = dir.join("empty.wal");

        // Create and immediately close
        {
            let db = GraphDb::new_with_wal(&wal_path).expect("Failed to create WAL graph");
            assert_eq!(db.node_count(), 0);
        }

        // Reopen with same WAL path
        {
            let db = GraphDb::new_with_wal(&wal_path).expect("Failed to reopen WAL graph");
            assert_eq!(db.node_count(), 0);
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ─── Compact ─────────────────────────────────────────────────────────

    #[test]
    fn test_compact_does_not_crash() {
        let db = create_test_graph();

        let a = db.create_node(vec!["A".to_string()], vec![]).unwrap();
        let b = db.create_node(vec!["B".to_string()], vec![]).unwrap();
        db.create_edge("e", a, b, vec![]).unwrap();

        // Compact should succeed without error
        db.compact().expect("Compaction failed");

        // Data should still be accessible after compaction
        assert_eq!(db.node_count(), 2);
        assert_eq!(db.edge_count(), 1);
    }

    // ─── Snapshot + Recovery ─────────────────────────────────────────────

    #[test]
    fn test_write_snapshot_creates_file() {
        let dir = std::env::temp_dir().join(format!("spire_test_snap_file_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let db = create_test_graph();
        let n1 = db.create_node(vec!["Snap".to_string()], vec![("k".to_string(), Value::String(to_db_string("v1")))]).unwrap();
        let n2 = db.create_node(vec!["Snap".to_string()], vec![("k".to_string(), Value::String(to_db_string("v2")))]).unwrap();
        db.create_edge("link", n1, n2, vec![]).unwrap();

        // Write snapshot at sequence 1
        let outcome = db.write_snapshot(&dir, 1, true).expect("Failed to write snapshot");
        assert_eq!(outcome.snapshot_seq, 1, "Snapshot sequence should match");

        // Verify snapshot file exists
        let snap_path = GraphDb::snapshot_path(&dir, 1);
        assert!(snap_path.exists(), "Snapshot file should exist: {:?}", snap_path);
        assert!(std::fs::metadata(&snap_path).unwrap().len() > 0, "Snapshot should have content");

        // Verify latest_snapshot_sequence returns the correct value
        let latest = GraphDb::latest_snapshot_sequence(&dir).expect("Failed to get latest sequence");
        assert_eq!(latest, Some(1), "Latest snapshot sequence should be 1");

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recover_from_snapshot() {
        let dir = std::env::temp_dir().join(format!("spire_test_snap_recover_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        // new_in_memory() uses GraphId::new(1), so we must match that on recovery
        let graph_id = GraphId::new(1);

        // Phase 1: create graph, write data, take snapshot
        {
            let db = GraphDb::new_in_memory().expect("Failed to create graph");
            let n1 = db.create_node(vec!["Recover".to_string()], vec![("name".to_string(), Value::String(to_db_string("alpha")))]).unwrap();
            let n2 = db.create_node(vec!["Recover".to_string()], vec![("name".to_string(), Value::String(to_db_string("beta")))]).unwrap();
            db.create_edge("relates", n1, n2, vec![]).unwrap();
            assert_eq!(db.node_count(), 2);
            assert_eq!(db.edge_count(), 1);

            // Write snapshot — this captures the current state
            db.write_snapshot(&dir, 1, true).expect("Failed to write snapshot");
        }

        // Phase 2: recover from snapshot
        {
            let db = GraphDb::recover(&dir, graph_id).expect("Failed to recover graph");
            assert_eq!(db.node_count(), 2, "Recovered graph should have 2 nodes");
            assert_eq!(db.edge_count(), 1, "Recovered graph should have 1 edge");

            // Verify node data survived
            let nodes = db.nodes_with_label("Recover");
            assert_eq!(nodes.len(), 2, "Should have 2 Recover nodes");
            let names: Vec<String> = nodes.iter()
                .filter_map(|n| n.properties.get("name"))
                .filter_map(|v| {
                    if let Value::String(s) = v { Some(s.to_string()) } else { None }
                })
                .collect();
            assert!(names.contains(&"alpha".to_string()), "Should contain alpha");
            assert!(names.contains(&"beta".to_string()), "Should contain beta");
        }

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recover_empty_dir_returns_empty_graph() {
        let dir = std::env::temp_dir().join(format!("spire_test_snap_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let graph_id = GraphId::new(1);
        let db = GraphDb::recover(&dir, graph_id).expect("Failed to recover from empty dir");
        assert_eq!(db.node_count(), 0, "Recovered empty graph should have 0 nodes");
        assert_eq!(db.edge_count(), 0, "Recovered empty graph should have 0 edges");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_latest_snapshot_sequence_empty_dir() {
        let dir = std::env::temp_dir().join(format!("spire_test_snap_latest_empty_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let latest = GraphDb::latest_snapshot_sequence(&dir).expect("Failed to get latest sequence");
        assert_eq!(latest, None, "Empty dir should have no snapshots");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_multiple_snapshots_returns_latest() {
        let dir = std::env::temp_dir().join(format!("spire_test_snap_multiple_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("Failed to create temp dir");

        let db = create_test_graph();

        // Write snapshot at sequence 1
        db.write_snapshot(&dir, 1, true).expect("Failed to write snapshot 1");

        // Add more data and write snapshot at sequence 2
        let _n = db.create_node(vec!["Later".to_string()], vec![]).unwrap();
        db.write_snapshot(&dir, 2, true).expect("Failed to write snapshot 2");

        // Latest should be 2
        let latest = GraphDb::latest_snapshot_sequence(&dir).expect("Failed to get latest sequence");
        assert_eq!(latest, Some(2), "Latest snapshot sequence should be 2");

        // Recover from the latest snapshot — should have the "Later" node
        let recovered = GraphDb::recover(&dir, db.graph_id()).expect("Failed to recover");
        assert_eq!(recovered.node_count(), 1, "Recovered from snapshot 2 should have 1 node");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_snapshot_path_format() {
        let dir = std::env::temp_dir().join("spire_test_snap_path");
        let path = GraphDb::snapshot_path(&dir, 42);
        let filename = path.file_name().unwrap().to_str().unwrap().to_string();
        assert!(filename.starts_with("snapshot."), "Filename should start with 'snapshot.'");
        assert!(filename.contains(".42."), "Filename should contain '.42.'");
        assert!(filename.ends_with(".snap"), "Filename should end with '.snap'");
    }
}
