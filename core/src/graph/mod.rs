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

//! Graph database wrapper around SeleneDB.
//!
//! This module provides a high-level wrapper around SeleneDB's graph database,
//! exposing a simplified API for node/edge CRUD, traversal, and vector search.
//! Persistence is handled via Write-Ahead Log (WAL).

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use selene_core::changeset::{SchemaChange, SchemaVectorIndexKind};

use selene_core::db_string::DbString;
use selene_core::identity::{EdgeId, GraphId, NodeId};
use selene_core::label_set::LabelSet;
use selene_core::property_map::PropertyMap;
use selene_core::value::Value;
use selene_core::VectorMetric;
use selene_core::VectorValue;
use selene_graph::graph::SeleneGraph;
use selene_graph::shared::SharedGraph;
use selene_persist::WalConfig;

/// A low-level node representation from SeleneDB.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SeleneNode {
    pub id: String,
    pub labels: Vec<String>,
    pub properties: std::collections::HashMap<String, Value>,
}

/// A low-level edge representation from SeleneDB.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SeleneEdge {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub properties: std::collections::HashMap<String, Value>,
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
        let graph = SeleneGraph::new(graph_id);
        let shared = SharedGraph::try_from_graph(graph)
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
        let graph_id = GraphId::new(1);
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

        let label_set = LabelSet::from_iter(
            labels.into_iter().map(|l| DbString::from_string(l).unwrap()),
        );
        let mut prop_map = PropertyMap::new();
        for (key, value) in properties {
            let key_str = key.clone();
            prop_map
                .set(DbString::from_string(key).unwrap(), value)
                .map_err(|e| anyhow::anyhow!("Failed to set property '{}': {}", key_str, e))?;
        }

        let node_id = mutator
            .create_node(label_set, prop_map)
            .map_err(|e| anyhow::anyhow!("Failed to create node: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit node creation: {}", e))?;

        Ok(node_id)
    }

    /// Get a node by its ID.
    pub fn get_node(&self, node_id: NodeId) -> Option<SeleneNode> {
        let snapshot = self.shared.read();

        let labels = snapshot.node_labels(node_id)?;
        let properties = snapshot.node_properties(node_id)?;

        Some(SeleneNode {
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
            let key_str = key.clone();
            prop_map
                .set(DbString::from_string(key).unwrap(), value)
                .map_err(|e| anyhow::anyhow!("Failed to set property '{}': {}", key_str, e))?;
        }

        let edge_id = mutator
            .create_edge(DbString::from_string(predicate.to_string()).unwrap(), subject, object, prop_map)
            .map_err(|e| anyhow::anyhow!("Failed to create edge: {}", e))?;

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit edge creation: {}", e))?;

        Ok(edge_id)
    }

    /// Get an edge by its ID.
    pub fn get_edge(&self, edge_id: EdgeId) -> Option<SeleneEdge> {
        let snapshot = self.shared.read();
        let label = snapshot.edge_label(edge_id)?;
        let endpoints = snapshot.edge_endpoints(edge_id)?;
        let properties = snapshot.edge_properties(edge_id)?;

        Some(SeleneEdge {
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
    pub fn outgoing_edges(&self, node_id: NodeId) -> Vec<SeleneEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        if let Some(adj) = snapshot.outgoing_edges(node_id) {
            for edge_ref in adj.edges.iter() {
                let edge_id = edge_ref.edge_id;
                if let Some(label) = snapshot.edge_label(edge_id) {
                    if let Some(endpoints) = snapshot.edge_endpoints(edge_id) {
                        let properties = snapshot.edge_properties(edge_id).cloned().unwrap_or_default();
                        edges.push(SeleneEdge {
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
    pub fn incoming_edges(&self, node_id: NodeId) -> Vec<SeleneEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        if let Some(adj) = snapshot.incoming_edges(node_id) {
            for edge_ref in adj.edges.iter() {
                let edge_id = edge_ref.edge_id;
                if let Some(label) = snapshot.edge_label(edge_id) {
                    if let Some(endpoints) = snapshot.edge_endpoints(edge_id) {
                        let properties = snapshot.edge_properties(edge_id).cloned().unwrap_or_default();
                        edges.push(SeleneEdge {
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
    pub fn nodes_with_label(&self, label: &str) -> Vec<SeleneNode> {
        let snapshot = self.shared.read();
        let mut nodes = Vec::new();

        let label_db = DbString::from_string(label.to_string()).unwrap();
        if let Some(bitmap) = snapshot.nodes_with_label(&label_db) {
            for raw_row in bitmap.iter() {
                let row = selene_graph::store::RowIndex::new(raw_row);
                if let Some(node_id) = snapshot.node_id_for_row(row) {
                    if let Some(node) = self.get_node(node_id) {
                        nodes.push(node);
                    }
                }
            }
        }

        nodes
    }

    /// Find edges by label.
    pub fn edges_with_label(&self, label: &str) -> Vec<SeleneEdge> {
        let snapshot = self.shared.read();
        let mut edges = Vec::new();

        let label_db = DbString::from_string(label.to_string()).unwrap();
        if let Some(bitmap) = snapshot.edges_with_label(&label_db) {
            for raw_row in bitmap.iter() {
                let row = selene_graph::store::RowIndex::new(raw_row);
                if let Some(edge_id) = snapshot.edge_id_for_row(row) {
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
        kind: SchemaVectorIndexKind,
    ) -> Result<()> {
        let mut txn = self.shared.begin_write();
        let mut mutator = txn.mutator();

        let change = SchemaChange::VectorIndexCreated {
            label: DbString::from_string(label.to_string()).unwrap(),
            property: DbString::from_string(property.to_string()).unwrap(),
            kind,
            dimension: dimensions,
            name: None,
            hnsw_config: None,
            ivf_config: None,
        };

        mutator.schema_change(self.graph_id, change);

        txn.commit()
            .map_err(|e| anyhow::anyhow!("Failed to commit vector index creation: {}", e))?;

        Ok(())
    }

    /// Perform an exact vector similarity search.
    ///
    /// Searches for nodes with the given label whose embedding property
    /// is closest to the query vector. Returns results sorted by distance
    /// (lower is more similar).
    pub fn vector_search(
        &self,
        label: &str,
        property: &str,
        query_vector: &[f32],
        limit: usize,
    ) -> Result<Vec<(NodeId, f64)>> {
        let snapshot = self.shared.read();

        let vector_value = VectorValue::new(query_vector.to_vec())
            .map_err(|e| anyhow::anyhow!("Failed to create vector value: {}", e))?;

        let hits = snapshot
            .exact_vector_search_nodes(
                &DbString::from_string(label.to_string()).unwrap(),
                &DbString::from_string(property.to_string()).unwrap(),
                &vector_value,
                VectorMetric::Cosine,
                limit,
            )
            .map_err(|e| anyhow::anyhow!("Vector search failed: {}", e))?;

        let results: Vec<(NodeId, f64)> = hits
            .iter()
            .map(|hit| (hit.node_id, hit.distance))
            .collect();

        Ok(results)
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
