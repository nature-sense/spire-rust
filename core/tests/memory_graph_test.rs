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

//! Unit tests for the `MemoryGraphActor` using its mailbox and messages.
//!
//! Each test creates a fresh `tonari_actor::System`, spawns a `MemoryGraphActor`
//! with an in-memory `GraphDb` and a `TestEmbedder`, then sends messages via
//! the actor's address and awaits replies via `oneshot` channels.
//!
//! This tests the actor's message handling directly — no MCP layer involved.

use std::collections::HashMap;
use std::sync::Arc;

use tonari_actor::{Actor, Addr};

use spire_rust::actors::memory_graph::{MemoryGraphActor, MemoryGraphMessage};

use spire_rust::graph::GraphDb;
use spire_rust::models::embedding::Embedder;
use spire_rust::models::memory_graph::{
    AgentContext, GraphEdge, GraphNode, MemoryMetadata, NodeFilter, NodeInput, NodeType, NodeUpdate,
    RelationshipInput, RelationshipType, SearchOptions, TraversalDirection, TraversalOptions,

};

// ============================================================================
// Test Infrastructure
// ============================================================================

/// A no-op embedder that returns a zero-filled 384-dimensional vector.
struct TestEmbedder;

#[async_trait::async_trait]
impl Embedder for TestEmbedder {
    async fn embed(&self, text: &str) -> anyhow::Result<spire_rust::models::embedding::Embedding> {
        Ok(spire_rust::models::embedding::Embedding::new(
            vec![0.0f32; 384],
            text,
            "test-model",
        ))
    }

    async fn embed_batch(
        &self,
        texts: &[String],
    ) -> anyhow::Result<Vec<spire_rust::models::embedding::Embedding>> {
        Ok(texts
            .iter()
            .map(|t| spire_rust::models::embedding::Embedding::new(vec![0.0f32; 384], t, "test-model"))
            .collect())
    }

    fn dimensions(&self) -> usize {
        384
    }
}

/// Create a fresh actor system with a MemoryGraphActor backed by an in-memory GraphDb.
fn setup() -> (Addr<MemoryGraphMessage>, tonari_actor::System) {
    let mut system = tonari_actor::System::new("memory-graph-test");
    let graph_db = Arc::new(GraphDb::new_in_memory().unwrap());
    let embedder = Arc::new(TestEmbedder);
    let addr = system.spawn(MemoryGraphActor::new(graph_db, embedder)).unwrap();
    (addr, system)
}

/// Helper: send a `StoreNode` message and await the reply.
async fn store_node(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::StoreNode {
        node: input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetNode` message and await the reply.
async fn get_node(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<Option<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetNode {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `QueryNodes` message and await the reply.
async fn query_nodes(
    addr: &Addr<MemoryGraphMessage>,
    filter: NodeFilter,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::QueryNodes {
        filter,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send an `UpdateNode` message and await the reply.
async fn update_node(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
    updates: NodeUpdate,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::UpdateNode {
        id: id.to_string(),
        updates,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `DeleteNode` message and await the reply.
async fn delete_node(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::DeleteNode {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `CreateRelationship` message and await the reply.
async fn create_relationship(
    addr: &Addr<MemoryGraphMessage>,
    rel: RelationshipInput,
) -> anyhow::Result<GraphEdge> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::CreateRelationship {
        rel,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetRelationships` message and await the reply.
async fn get_relationships(
    addr: &Addr<MemoryGraphMessage>,
    node_id: &str,
) -> anyhow::Result<Vec<GraphEdge>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetRelationships {
        node_id: node_id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `DeleteRelationship` message and await the reply.
async fn delete_relationship(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::DeleteRelationship {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `Traverse` message and await the reply.
async fn traverse(
    addr: &Addr<MemoryGraphMessage>,
    start_node_id: &str,
    options: TraversalOptions,
) -> anyhow::Result<spire_rust::models::memory_graph::TraversalResult> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::Traverse {
        start_node_id: start_node_id.to_string(),
        options,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetProjectContext` message and await the reply.
async fn get_project_context(
    addr: &Addr<MemoryGraphMessage>,
) -> anyhow::Result<spire_rust::models::memory_graph::ProjectSnapshot> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetProjectContext { reply_to: tx })
        .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `SearchContext` message and await the reply.
async fn search_context(
    addr: &Addr<MemoryGraphMessage>,
    query: &str,
    options: Option<SearchOptions>,
) -> anyhow::Result<spire_rust::models::memory_graph::ContextSearchResult> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::SearchContext {
        query: query.to_string(),
        options,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send an `AddMemory` message and await the reply.
async fn add_memory(
    addr: &Addr<MemoryGraphMessage>,
    text: &str,
    metadata: Option<MemoryMetadata>,
) -> anyhow::Result<spire_rust::models::memory_graph::MemoryEntry> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::AddMemory {
        text: text.to_string(),
        metadata,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `Recall` message and await the reply.
async fn recall(
    addr: &Addr<MemoryGraphMessage>,
    query: &str,
    limit: Option<usize>,
) -> anyhow::Result<Vec<spire_rust::models::memory_graph::MemoryEntry>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::Recall {
        query: query.to_string(),
        limit,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `Sync` message and await the reply.
async fn sync_graph(
    addr: &Addr<MemoryGraphMessage>,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::Sync { reply_to: tx })
        .unwrap();
    rx.await.unwrap()
}

// ============================================================================
// Node Operation Tests
// ============================================================================

#[tokio::test]
async fn test_store_and_get_node() {
    let (addr, _system) = setup();

    let input = NodeInput {
        node_type: NodeType::Entity,
        subtype: Some("Function".to_string()),
        name: "calculate_total".to_string(),
        description: Some("Computes the total from line items.".to_string()),
        properties: Some(HashMap::from([(
            "language".to_string(),
            serde_json::json!("rust"),
        )])),
        embedding_id: None,
    };

    let stored = store_node(&addr, input.clone()).await.unwrap();

    // Verify stored node has all fields populated
    assert!(!stored.id.is_empty(), "Node should have a UUID");
    assert_eq!(stored.node_type, NodeType::Entity);
    assert_eq!(stored.subtype, Some("Function".to_string()));
    assert_eq!(stored.name, "calculate_total");
    assert_eq!(
        stored.description.as_deref(),
        Some("Computes the total from line items.")
    );
    assert_eq!(stored.version, 1);
    assert_eq!(
        stored.properties.get("language").and_then(|v| v.as_str()),
        Some("rust")
    );

    // Retrieve by ID
    let retrieved = get_node(&addr, &stored.id).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id, stored.id);
    assert_eq!(retrieved.name, "calculate_total");
}

#[tokio::test]
async fn test_store_duplicate_node_fails() {
    let (addr, _system) = setup();

    let input = NodeInput {
        node_type: NodeType::Entity,
        subtype: None,
        name: "unique_name".to_string(),
        description: None,
        properties: None,
        embedding_id: None,
    };

    // First store should succeed
    store_node(&addr, input.clone()).await.unwrap();

    // Second store with same (type, name) should fail
    let result = store_node(&addr, input.clone()).await;
    assert!(result.is_err(), "Duplicate node should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Duplicate node") || err.contains("already exists"),
        "Error should mention duplicate: {}",
        err
    );
}

#[tokio::test]
async fn test_query_nodes_by_type() {
    let (addr, _system) = setup();

    // Store nodes of different types
    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "entity_1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "entity_2".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Decision,
            name: "decision_1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Query for Entity nodes only
    let entities = query_nodes(
        &addr,
        NodeFilter {
            node_type: Some(NodeType::Entity),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(entities.len(), 2, "Should find 2 Entity nodes");
    assert!(entities.iter().all(|n| n.node_type == NodeType::Entity));

    // Query for Decision nodes only
    let decisions = query_nodes(
        &addr,
        NodeFilter {
            node_type: Some(NodeType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(decisions.len(), 1, "Should find 1 Decision node");
}

#[tokio::test]
async fn test_query_nodes_by_name() {
    let (addr, _system) = setup();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "MyFunction".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "OtherClass".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Case-insensitive name search
    let results = query_nodes(
        &addr,
        NodeFilter {
            name: Some("function".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(results.len(), 1, "Should find 1 node matching 'function'");
    assert_eq!(results[0].name, "MyFunction");
}

#[tokio::test]
async fn test_update_node() {
    let (addr, _system) = setup();

    let stored = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "old_name".to_string(),
            description: Some("Original description.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(stored.version, 1);

    // Update name and description
    let updated = update_node(
        &addr,
        &stored.id,
        NodeUpdate {
            name: Some("new_name".to_string()),
            description: Some(Some("Updated description.".to_string())),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.name, "new_name");
    assert_eq!(
        updated.description.as_deref(),
        Some("Updated description.")
    );
    assert_eq!(updated.version, 2, "Version should increment on update");
    assert!(
        updated.updated_at > stored.updated_at,
        "updated_at should advance"
    );

    // Verify the change persisted
    let retrieved = get_node(&addr, &stored.id).await.unwrap().unwrap();
    assert_eq!(retrieved.name, "new_name");
    assert_eq!(retrieved.version, 2);
}

#[tokio::test]
async fn test_update_nonexistent_node_fails() {
    let (addr, _system) = setup();

    let result = update_node(
        &addr,
        "nonexistent-uuid",
        NodeUpdate {
            name: Some("anything".to_string()),
            ..Default::default()
        },
    )
    .await;

    assert!(result.is_err(), "Updating a nonexistent node should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "Error should mention 'not found': {}",
        err
    );
}

#[tokio::test]
async fn test_delete_node() {
    let (addr, _system) = setup();

    let stored = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "to_delete".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Verify it exists
    assert!(get_node(&addr, &stored.id).await.unwrap().is_some());

    // Delete it
    delete_node(&addr, &stored.id).await.unwrap();

    // Verify it's gone
    let retrieved = get_node(&addr, &stored.id).await.unwrap();
    assert!(retrieved.is_none(), "Deleted node should not be found");
}

#[tokio::test]
async fn test_delete_node_cascades_edges() {
    let (addr, _system) = setup();

    // Create two nodes
    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "node_a".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "node_b".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create a relationship between them
    let edge = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // Verify the edge exists
    let edges = get_relationships(&addr, &node_a.id).await.unwrap();
    assert_eq!(edges.len(), 1);

    // Delete node_a
    delete_node(&addr, &node_a.id).await.unwrap();

    // The edge should be gone too
    let edges_after = get_relationships(&addr, &node_b.id).await.unwrap();
    assert_eq!(
        edges_after.len(),
        0,
        "Edges should be cascaded on node deletion"
    );

    // The edge itself should not be retrievable
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::DeleteRelationship {
        id: edge.id.clone(),
        reply_to: tx,
    })
    .unwrap();
    // Deleting a non-existent edge should still succeed (idempotent)
    let result = rx.await.unwrap();
    assert!(result.is_ok());
}

// ============================================================================
// Relationship Operation Tests
// ============================================================================

#[tokio::test]
async fn test_create_and_get_relationship() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "source".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "target".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: Some(HashMap::from([(
                "reason".to_string(),
                serde_json::json!("test dependency"),
            )])),
            weight: Some(0.5),
        },
    )
    .await
    .unwrap();

    assert!(!edge.id.is_empty(), "Edge should have a UUID");
    assert_eq!(edge.edge_type, RelationshipType::DependsOn);
    assert_eq!(edge.from_id, node_a.id);
    assert_eq!(edge.to_id, node_b.id);
    assert_eq!(edge.weight, Some(0.5));

    // Get relationships from source node
    let edges = get_relationships(&addr, &node_a.id).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].id, edge.id);

    // Get relationships from target node (should also appear)
    let edges = get_relationships(&addr, &node_b.id).await.unwrap();
    assert_eq!(edges.len(), 1);
}

#[tokio::test]
async fn test_create_relationship_missing_from() {
    let (addr, _system) = setup();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "target".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: "nonexistent-from".to_string(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await;

    assert!(result.is_err(), "Missing source node should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "Error should mention 'not found': {}",
        err
    );
}

#[tokio::test]
async fn test_create_relationship_missing_to() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "source".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: node_a.id.clone(),
            to_id: "nonexistent-to".to_string(),
            properties: None,
            weight: None,
        },
    )
    .await;

    assert!(result.is_err(), "Missing target node should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "Error should mention 'not found': {}",
        err
    );
}

#[tokio::test]
async fn test_delete_relationship() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "a".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "b".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // Verify edge exists
    assert_eq!(
        get_relationships(&addr, &node_a.id).await.unwrap().len(),
        1
    );

    // Delete the edge
    delete_relationship(&addr, &edge.id).await.unwrap();

    // Verify it's gone
    let edges = get_relationships(&addr, &node_a.id).await.unwrap();
    assert_eq!(edges.len(), 0, "Edge should be removed after deletion");
}

#[tokio::test]
async fn test_acyclic_dependency_enforcement() {
    let (addr, _system) = setup();

    // Create nodes A, B, C
    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "A".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "B".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_c = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "C".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // A -> B (depends_on)
    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // B -> C (depends_on)
    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_b.id.clone(),
            to_id: node_c.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // C -> A (depends_on) — this should create a cycle and be rejected
    let result = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_c.id.clone(),
            to_id: node_a.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await;

    assert!(
        result.is_err(),
        "Cyclic dependency should be rejected"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("cycle") || err.contains("Acyclic"),
        "Error should mention cycle: {}",
        err
    );

    // Self-loop should also be rejected
    let self_loop = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_a.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await;

    assert!(
        self_loop.is_err(),
        "Self-loop dependency should be rejected"
    );
}

// ============================================================================
// Traversal Tests
// ============================================================================

#[tokio::test]
async fn test_traverse_single_node() {
    let (addr, _system) = setup();

    let node = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "lonely_node".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let result = traverse(
        &addr,
        &node.id,
        TraversalOptions {
            max_depth: 10,
            relationship_types: None,
            max_nodes: Some(100),
            direction: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 1, "Should return only the start node");
    assert_eq!(result.nodes[0].id, node.id);
    assert!(result.edges.is_empty(), "No edges expected");
}

#[tokio::test]
async fn test_traverse_chain() {
    let (addr, _system) = setup();

    // Create a chain: A -> B -> C -> D
    let nodes: Vec<GraphNode> = futures::future::join_all(["A", "B", "C", "D"].iter().map(|name| {
        store_node(
            &addr,
            NodeInput {
                node_type: NodeType::Entity,
                name: name.to_string(),
                ..Default::default()
            },
        )
    }))
    .await
    .into_iter()
    .map(|r| r.unwrap())
    .collect();


    // Link them
    for i in 0..3 {
        create_relationship(
            &addr,
            RelationshipInput {
                edge_type: RelationshipType::DependsOn,
                from_id: nodes[i].id.clone(),
                to_id: nodes[i + 1].id.clone(),
                properties: None,
                weight: None,
            },
        )
        .await
        .unwrap();
    }

    // Traverse from A with depth 10
    let result = traverse(
        &addr,
        &nodes[0].id,
        TraversalOptions {
            max_depth: 10,
            relationship_types: None,
            max_nodes: Some(100),
            direction: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 4, "Should find all 4 nodes");
    assert_eq!(result.edges.len(), 3, "Should find all 3 edges");
}

#[tokio::test]
async fn test_traverse_max_depth() {
    let (addr, _system) = setup();

    // Create a chain: A -> B -> C -> D
    let nodes: Vec<GraphNode> = futures::future::join_all(["A", "B", "C", "D"].iter().map(|name| {
        store_node(
            &addr,
            NodeInput {
                node_type: NodeType::Entity,
                name: name.to_string(),
                ..Default::default()
            },
        )
    }))
    .await
    .into_iter()
    .map(|r| r.unwrap())
    .collect();


    for i in 0..3 {
        create_relationship(
            &addr,
            RelationshipInput {
                edge_type: RelationshipType::DependsOn,
                from_id: nodes[i].id.clone(),
                to_id: nodes[i + 1].id.clone(),
                properties: None,
                weight: None,
            },
        )
        .await
        .unwrap();
    }

    // Traverse with depth 1 — should only get A and B
    let result = traverse(
        &addr,
        &nodes[0].id,
        TraversalOptions {
            max_depth: 1,
            relationship_types: None,
            max_nodes: Some(100),
            direction: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 2, "Depth 1 should find 2 nodes (A, B)");
    assert_eq!(result.edges.len(), 1, "Depth 1 should find 1 edge (A->B)");
}

#[tokio::test]
async fn test_traverse_direction_out() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "A".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "B".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // A -> B
    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // Traverse from B with Out direction — should find nothing (B has no outgoing edges)
    let result = traverse(
        &addr,
        &node_b.id,
        TraversalOptions {
            max_depth: 10,
            relationship_types: None,
            max_nodes: Some(100),
            direction: Some(TraversalDirection::Out),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 1, "Only B itself with Out direction");
}

#[tokio::test]
async fn test_traverse_direction_in() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "A".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "B".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // A -> B
    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // Traverse from B with In direction — should find A
    let result = traverse(
        &addr,
        &node_b.id,
        TraversalOptions {
            max_depth: 10,
            relationship_types: None,
            max_nodes: Some(100),
            direction: Some(TraversalDirection::In),
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 2, "B and A with In direction");
    assert_eq!(result.edges.len(), 1);
}

// ============================================================================
// Context & Memory Tests
// ============================================================================

#[tokio::test]
async fn test_get_project_context_empty() {
    let (addr, _system) = setup();

    let snapshot = get_project_context(&addr).await.unwrap();

    // With no project node stored, should return a default placeholder
    assert_eq!(snapshot.project.name, "Untitled Project");
    assert_eq!(
        snapshot.project.description.as_deref(),
        Some("No project context available.")
    );
    assert_eq!(snapshot.stats.total_nodes, 0);
    assert_eq!(snapshot.stats.total_relationships, 0);
}

#[tokio::test]
async fn test_get_project_context_with_project_node() {
    let (addr, _system) = setup();

    // Store a project node
    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Project,
            name: "My Project".to_string(),
            description: Some("A test project.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let snapshot = get_project_context(&addr).await.unwrap();

    assert_eq!(snapshot.project.name, "My Project");
    assert_eq!(
        snapshot.project.description.as_deref(),
        Some("A test project.")
    );
    assert_eq!(snapshot.stats.total_nodes, 1);
}

#[tokio::test]
async fn test_search_context_text_fallback() {
    let (addr, _system) = setup();

    // Store nodes with descriptions
    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "PaymentProcessor".to_string(),
            description: Some("Handles payment transactions and refunds.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "UserAuth".to_string(),
            description: Some("Manages user authentication and sessions.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Search for "payment" — should find PaymentProcessor via text fallback
    let result = search_context(&addr, "payment", None).await.unwrap();

    assert!(
        !result.nodes.is_empty(),
        "Should find at least one node matching 'payment'"
    );
    let names: Vec<&str> = result.nodes.iter().map(|s| s.node.name.as_str()).collect();
    assert!(
        names.contains(&"PaymentProcessor"),
        "Should find PaymentProcessor: {:?}",
        names
    );
}

#[tokio::test]
async fn test_search_context_with_options() {
    let (addr, _system) = setup();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "DataLoader".to_string(),
            description: Some("Loads data from external sources.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Search with top_k limit
    let result = search_context(
        &addr,
        "data",
        Some(SearchOptions {
            top_k: Some(1),
            ..Default::default()
        }),
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 1, "top_k=1 should return 1 result");
    assert_eq!(result.nodes[0].node.name, "DataLoader");
}

#[tokio::test]
async fn test_add_memory() {
    let (addr, _system) = setup();

    let entry = add_memory(
        &addr,
        "Remember to refactor the payment module.",
        Some(MemoryMetadata {
            mem_type: Some(NodeType::Conversation),
            tags: Some(vec!["refactor".to_string(), "payment".to_string()]),
            source: Some("user".to_string()),
            confidence: Some(0.9),
        }),
    )
    .await
    .unwrap();

    assert!(!entry.id.is_empty(), "Memory entry should have an ID");
    assert_eq!(entry.text, "Remember to refactor the payment module.");
    assert_eq!(entry.metadata.mem_type, Some(NodeType::Conversation));
    assert_eq!(
        entry.metadata.tags,
        Some(vec!["refactor".to_string(), "payment".to_string()])
    );
    assert_eq!(entry.metadata.source, Some("user".to_string()));
    assert_eq!(entry.metadata.confidence, Some(0.9));
}

#[tokio::test]
async fn test_recall() {
    let (addr, _system) = setup();

    // Store nodes that should be recallable
    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "AuthService".to_string(),
            description: Some("Handles user authentication.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "PaymentService".to_string(),
            description: Some("Processes payments.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Recall by name
    let entries = recall(&addr, "Auth", Some(5)).await.unwrap();
    assert_eq!(entries.len(), 1, "Should recall 1 entry matching 'Auth'");
    assert_eq!(entries[0].text, "Handles user authentication.");

    // Recall by description text
    let entries = recall(&addr, "payments", Some(5)).await.unwrap();
    assert_eq!(entries.len(), 1, "Should recall 1 entry matching 'payments'");
    assert_eq!(entries[0].text, "Processes payments.");
}

#[tokio::test]
async fn test_recall_with_limit() {
    let (addr, _system) = setup();

    // Store multiple nodes
    for i in 0..5 {
        store_node(
            &addr,
            NodeInput {
                node_type: NodeType::Entity,
                name: format!("Item_{}", i),
                description: Some(format!("Description for item {}.", i)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    // Recall with limit 2
    let entries = recall(&addr, "item", Some(2)).await.unwrap();
    assert_eq!(entries.len(), 2, "Limit of 2 should return 2 entries");
}

// ============================================================================
// Maintenance Tests
// ============================================================================

#[tokio::test]
async fn test_sync_succeeds() {
    let (addr, _system) = setup();

    let result = sync_graph(&addr).await;
    assert!(result.is_ok(), "Sync should always succeed");
}

// ============================================================================
// Edge Cases & Error Handling
// ============================================================================

#[tokio::test]
async fn test_get_nonexistent_node() {
    let (addr, _system) = setup();

    let result = get_node(&addr, "nonexistent-uuid").await.unwrap();
    assert!(result.is_none(), "Nonexistent node should return None");
}

#[tokio::test]
async fn test_delete_nonexistent_node() {
    let (addr, _system) = setup();

    // Deleting a non-existent node should succeed (idempotent)
    let result = delete_node(&addr, "nonexistent-uuid").await;
    assert!(result.is_ok(), "Deleting nonexistent node should succeed");
}

#[tokio::test]
async fn test_delete_nonexistent_relationship() {
    let (addr, _system) = setup();

    let result = delete_relationship(&addr, "nonexistent-edge-uuid").await;
    assert!(
        result.is_ok(),
        "Deleting nonexistent relationship should succeed"
    );
}

#[tokio::test]
async fn test_traverse_nonexistent_start_node() {
    let (addr, _system) = setup();

    let result = traverse(
        &addr,
        "nonexistent-node",
        TraversalOptions {
            max_depth: 5,
            relationship_types: None,
            max_nodes: Some(100),
            direction: None,
        },
    )
    .await
    .unwrap();

    // Traversal of a nonexistent node should return just the start node
    // (the traverse method doesn't error — it just returns what it finds)
    assert_eq!(result.nodes.len(), 0, "No nodes should be found");
    assert!(result.edges.is_empty());
}

#[tokio::test]
async fn test_store_node_without_optional_fields() {
    let (addr, _system) = setup();

    // Store a node with only required fields
    let stored = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "minimal_node".to_string(),
            subtype: None,
            description: None,
            properties: None,
            embedding_id: None,
        },
    )
    .await
    .unwrap();

    assert!(!stored.id.is_empty());
    assert_eq!(stored.name, "minimal_node");
    assert_eq!(stored.node_type, NodeType::Entity);
    assert!(stored.subtype.is_none());
    assert!(stored.description.is_none());
    assert!(stored.properties.is_empty());
    assert!(stored.embedding_id.is_none());
    assert_eq!(stored.version, 1);
}

#[tokio::test]
async fn test_update_node_clear_description() {
    let (addr, _system) = setup();

    let stored = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "clear_desc_test".to_string(),
            description: Some("Will be cleared.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Clear the description
    let updated = update_node(
        &addr,
        &stored.id,
        NodeUpdate {
            description: Some(None), // Explicitly set to null
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(updated.description.is_none(), "Description should be cleared");
    assert_eq!(updated.version, 2);
}

#[tokio::test]
async fn test_update_node_change_type() {
    let (addr, _system) = setup();

    let stored = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "type_change_test".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Change the node type
    let updated = update_node(
        &addr,
        &stored.id,
        NodeUpdate {
            node_type: Some(NodeType::Decision),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(updated.node_type, NodeType::Decision);
    assert_eq!(updated.version, 2);
}

#[tokio::test]
async fn test_multiple_relationships_between_same_nodes() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "multi_rel_a".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "multi_rel_b".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create two different relationship types between the same nodes
    let edge1 = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    let edge2 = create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    assert_ne!(edge1.id, edge2.id, "Two edges should have different IDs");

    let edges = get_relationships(&addr, &node_a.id).await.unwrap();
    assert_eq!(edges.len(), 2, "Should have 2 relationships");
}

#[tokio::test]
async fn test_traverse_with_relationship_type_filter() {
    let (addr, _system) = setup();

    let node_a = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "filter_test_a".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_b = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "filter_test_b".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let node_c = store_node(
        &addr,
        NodeInput {
            node_type: NodeType::Entity,
            name: "filter_test_c".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // A -> B (depends_on), A -> C (belongs_to)
    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::DependsOn,
            from_id: node_a.id.clone(),
            to_id: node_b.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    create_relationship(
        &addr,
        RelationshipInput {
            edge_type: RelationshipType::BelongsTo,
            from_id: node_a.id.clone(),
            to_id: node_c.id.clone(),
            properties: None,
            weight: None,
        },
    )
    .await
    .unwrap();

    // Traverse with only DependsOn filter
    let result = traverse(
        &addr,
        &node_a.id,
        TraversalOptions {
            max_depth: 10,
            relationship_types: Some(vec![RelationshipType::DependsOn]),
            max_nodes: Some(100),
            direction: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.nodes.len(), 2, "Should find A and B (depends_on only)");
    assert_eq!(result.edges.len(), 1, "Should find 1 edge (depends_on)");
    assert_eq!(result.edges[0].edge_type, RelationshipType::DependsOn);
}

// ============================================================================
// Agent Infrastructure Tests
// ============================================================================

// ── Agent Management ────────────────────────────────────────────────────────

/// Helper: send a `CreateAgent` message and await the reply.
async fn create_agent(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::CreateAgent {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetAgent` message and await the reply.
async fn get_agent(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetAgent {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetAgentByName` message and await the reply.
async fn get_agent_by_name(
    addr: &Addr<MemoryGraphMessage>,
    name: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetAgentByName {
        name: name.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetActiveAgents` message and await the reply.
async fn get_active_agents(
    addr: &Addr<MemoryGraphMessage>,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetActiveAgents { reply_to: tx })
        .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetAgentContext` message and await the reply.
async fn get_agent_context(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
    goal: &str,
) -> anyhow::Result<AgentContext> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetAgentContext {
        agent_id: agent_id.to_string(),
        goal: goal.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `CreateTool` message and await the reply.
async fn create_tool(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::CreateTool {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetTool` message and await the reply.
async fn get_tool(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetTool {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetToolsForAgent` message and await the reply.
async fn get_tools_for_agent(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetToolsForAgent {
        agent_id: agent_id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetToolByName` message and await the reply.
async fn get_tool_by_name(
    addr: &Addr<MemoryGraphMessage>,
    name: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetToolByName {
        name: name.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `CreatePlan` message and await the reply.
async fn create_plan(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::CreatePlan {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetPlan` message and await the reply.
async fn get_plan(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetPlan {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetPlanSteps` message and await the reply.
async fn get_plan_steps(
    addr: &Addr<MemoryGraphMessage>,
    plan_id: &str,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetPlanSteps {
        plan_id: plan_id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetNextStep` message and await the reply.
async fn get_next_step(
    addr: &Addr<MemoryGraphMessage>,
    plan_id: &str,
    current_order: u32,
) -> anyhow::Result<Option<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetNextStep {
        plan_id: plan_id.to_string(),
        current_order,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `StartExecution` message and await the reply.
async fn start_execution(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::StartExecution {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send an `UpdateExecutionStatus` message and await the reply.
async fn update_execution_status(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
    status: &str,
    result: Option<String>,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::UpdateExecutionStatus {
        id: id.to_string(),
        status: status.to_string(),
        result,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetExecution` message and await the reply.
async fn get_execution(
    addr: &Addr<MemoryGraphMessage>,
    id: &str,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetExecution {
        id: id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetExecutionHistory` message and await the reply.
async fn get_execution_history(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetExecutionHistory {
        agent_id: agent_id.to_string(),
        limit,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetSuccessfulExecutions` message and await the reply.
async fn get_successful_executions(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetSuccessfulExecutions {
        agent_id: agent_id.to_string(),
        limit,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetFailedExecutions` message and await the reply.
async fn get_failed_executions(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetFailedExecutions {
        agent_id: agent_id.to_string(),
        limit,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `RecordTaskResult` message and await the reply.
async fn record_task_result(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::RecordTaskResult {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetTaskResults` message and await the reply.
async fn get_task_results(
    addr: &Addr<MemoryGraphMessage>,
    execution_id: &str,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetTaskResults {
        execution_id: execution_id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `RecordArtifact` message and await the reply.
async fn record_artifact(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::RecordArtifact {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetArtifacts` message and await the reply.
async fn get_artifacts(
    addr: &Addr<MemoryGraphMessage>,
    execution_id: &str,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetArtifacts {
        execution_id: execution_id.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetLatestArtifact` message and await the reply.
async fn get_latest_artifact(
    addr: &Addr<MemoryGraphMessage>,
    agent_id: &str,
    artifact_type: &str,
) -> anyhow::Result<Option<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetLatestArtifact {
        agent_id: agent_id.to_string(),
        artifact_type: artifact_type.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `RecordError` message and await the reply.
async fn record_error(
    addr: &Addr<MemoryGraphMessage>,
    input: NodeInput,
) -> anyhow::Result<GraphNode> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::RecordError {
        input,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetErrorByFingerprint` message and await the reply.
async fn get_error_by_fingerprint(
    addr: &Addr<MemoryGraphMessage>,
    fingerprint: &str,
) -> anyhow::Result<Option<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetErrorByFingerprint {
        fingerprint: fingerprint.to_string(),
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `GetSimilarErrors` message and await the reply.
async fn get_similar_errors(
    addr: &Addr<MemoryGraphMessage>,
    embedding_id: &str,
    limit: usize,
) -> anyhow::Result<Vec<GraphNode>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::GetSimilarErrors {
        embedding_id: embedding_id.to_string(),
        limit,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a `LinkErrorToFix` message and await the reply.
async fn link_error_to_fix(
    addr: &Addr<MemoryGraphMessage>,
    error_id: &str,
    execution_id: &str,
    properties: HashMap<String, serde_json::Value>,
) -> anyhow::Result<()> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    addr.send(MemoryGraphMessage::LinkErrorToFix {
        error_id: error_id.to_string(),
        execution_id: execution_id.to_string(),
        properties,
        reply_to: tx,
    })
    .unwrap();
    rx.await.unwrap()
}

/// Helper: send a relationship creation message for agent-specific relationship types.
/// Takes a message factory closure that receives the oneshot sender.
async fn create_agent_relationship<F>(
    addr: &Addr<MemoryGraphMessage>,
    make_msg: F,
) -> anyhow::Result<GraphEdge>
where
    F: FnOnce(tokio::sync::oneshot::Sender<anyhow::Result<GraphEdge>>) -> MemoryGraphMessage,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let msg = make_msg(tx);
    addr.send(msg).unwrap();
    rx.await.unwrap()
}


// ============================================================================
// Agent Management Tests
// ============================================================================

#[tokio::test]
async fn test_create_agent() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "CodeReviewer".to_string(),
            description: Some("Reviews pull requests for code quality.".to_string()),
            properties: Some(HashMap::from([
                ("model".to_string(), serde_json::json!("gpt-4")),
                ("is_active".to_string(), serde_json::json!(true)),
            ])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!agent.id.is_empty());
    assert_eq!(agent.node_type, NodeType::Agent);
    assert_eq!(agent.name, "CodeReviewer");
    assert_eq!(
        agent.description.as_deref(),
        Some("Reviews pull requests for code quality.")
    );
    assert_eq!(
        agent.properties.get("model").and_then(|v| v.as_str()),
        Some("gpt-4")
    );
    assert_eq!(
        agent.properties.get("is_active").and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(agent.version, 1);
}

#[tokio::test]
async fn test_get_agent_by_id() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "TestAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retrieved = get_agent(&addr, &agent.id).await.unwrap();
    assert_eq!(retrieved.id, agent.id);
    assert_eq!(retrieved.name, "TestAgent");
    assert_eq!(retrieved.node_type, NodeType::Agent);
}

#[tokio::test]
async fn test_get_agent_by_name() {
    let (addr, _system) = setup();

    create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "UniqueAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retrieved = get_agent_by_name(&addr, "UniqueAgent").await.unwrap();
    assert_eq!(retrieved.name, "UniqueAgent");
    assert_eq!(retrieved.node_type, NodeType::Agent);
}

#[tokio::test]
async fn test_get_agent_by_name_not_found() {
    let (addr, _system) = setup();

    let result = get_agent_by_name(&addr, "NonExistentAgent").await;
    assert!(result.is_err(), "Non-existent agent should error");
}

#[tokio::test]
async fn test_get_agent_not_found() {
    let (addr, _system) = setup();

    let result = get_agent(&addr, "nonexistent-agent-id").await;
    assert!(result.is_err(), "Non-existent agent should error");
}

#[tokio::test]
async fn test_get_active_agents() {
    let (addr, _system) = setup();

    // Create an active agent
    create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "ActiveAgent".to_string(),
            properties: Some(HashMap::from([(
                "is_active".to_string(),
                serde_json::json!(true),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create an inactive agent
    create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "InactiveAgent".to_string(),
            properties: Some(HashMap::from([(
                "is_active".to_string(),
                serde_json::json!(false),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create an agent without is_active property
    create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "NoPropAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let active = get_active_agents(&addr).await.unwrap();
    assert_eq!(active.len(), 1, "Only 1 agent should be active");
    assert_eq!(active[0].name, "ActiveAgent");
}

#[tokio::test]
async fn test_get_agent_context_empty() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "LonelyAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let context = get_agent_context(&addr, &agent.id, "Fix bugs").await.unwrap();

    assert_eq!(context.agent.id, agent.id);
    assert_eq!(context.current_goal, "Fix bugs");
    assert!(context.tools.is_empty(), "No tools should be associated");
    assert!(context.plan.is_none(), "No plan should be associated");
    assert!(context.steps.is_empty());
    assert!(context.recent_successes.is_empty());
    assert!(context.artifacts.is_empty());
}

#[tokio::test]
async fn test_get_agent_context_with_tools_and_plan() {
    let (addr, _system) = setup();

    // Create agent
    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "BuilderAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create tools
    let tool1 = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "read_file".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tool2 = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "write_file".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link tools to agent
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateUsesTool {
            agent_id: agent.id.clone(),
            tool_id: tool1.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateUsesTool {
            agent_id: agent.id.clone(),
            tool_id: tool2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // Create a plan
    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "BuildFeature".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link agent to plan
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateFollowsPlan {
            agent_id: agent.id.clone(),
            plan_id: plan.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // Create plan steps
    let step1 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 1: Design".to_string(),
            properties: Some(HashMap::from([(
                "step_order".to_string(),
                serde_json::json!(1),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step2 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 2: Implement".to_string(),
            properties: Some(HashMap::from([(
                "step_order".to_string(),
                serde_json::json!(2),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link steps to plan
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step1.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();


    // Get context
    let context = get_agent_context(&addr, &agent.id, "Build the feature").await.unwrap();

    assert_eq!(context.tools.len(), 2, "Should have 2 tools");
    let tool_names: Vec<&str> = context.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(tool_names.contains(&"read_file"));
    assert!(tool_names.contains(&"write_file"));

    assert!(context.plan.is_some(), "Should have a plan");
    assert_eq!(context.plan.unwrap().name, "BuildFeature");

    assert_eq!(context.steps.len(), 2, "Should have 2 steps");
    assert_eq!(context.current_goal, "Build the feature");
}

#[tokio::test]
async fn test_get_agent_context_nonexistent_agent() {
    let (addr, _system) = setup();

    let result = get_agent_context(&addr, "bad-id", "test").await;
    assert!(result.is_err(), "Non-existent agent should error");
}

// ============================================================================
// Tool Management Tests
// ============================================================================

#[tokio::test]
async fn test_create_tool() {
    let (addr, _system) = setup();

    let tool = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "search_code".to_string(),
            description: Some("Searches codebase for patterns.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!tool.id.is_empty());
    assert_eq!(tool.node_type, NodeType::Tool);
    assert_eq!(tool.name, "search_code");
    assert_eq!(
        tool.description.as_deref(),
        Some("Searches codebase for patterns.")
    );
}

#[tokio::test]
async fn test_get_tool_by_id() {
    let (addr, _system) = setup();

    let tool = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "my_tool".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retrieved = get_tool(&addr, &tool.id).await.unwrap();
    assert_eq!(retrieved.id, tool.id);
    assert_eq!(retrieved.name, "my_tool");
}

#[tokio::test]
async fn test_get_tool_not_found() {
    let (addr, _system) = setup();

    let result = get_tool(&addr, "nonexistent-tool").await;
    assert!(result.is_err(), "Non-existent tool should error");
}

#[tokio::test]
async fn test_get_tool_by_name() {
    let (addr, _system) = setup();

    create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "FindTool".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retrieved = get_tool_by_name(&addr, "FindTool").await.unwrap();
    assert_eq!(retrieved.name, "FindTool");
    assert_eq!(retrieved.node_type, NodeType::Tool);
}

#[tokio::test]
async fn test_get_tool_by_name_not_found() {
    let (addr, _system) = setup();

    let result = get_tool_by_name(&addr, "NonExistentTool").await;
    assert!(result.is_err(), "Non-existent tool should error");
}

#[tokio::test]
async fn test_get_tools_for_agent() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "ToolUser".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tool_a = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "ToolA".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tool_b = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "ToolB".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link both tools to agent
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateUsesTool {
            agent_id: agent.id.clone(),
            tool_id: tool_a.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateUsesTool {
            agent_id: agent.id.clone(),
            tool_id: tool_b.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let tools = get_tools_for_agent(&addr, &agent.id).await.unwrap();
    assert_eq!(tools.len(), 2);
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"ToolA"));
    assert!(names.contains(&"ToolB"));
}

#[tokio::test]
async fn test_get_tools_for_agent_empty() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "ToolLessAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tools = get_tools_for_agent(&addr, &agent.id).await.unwrap();
    assert!(tools.is_empty(), "Agent with no tools should return empty list");
}

// ============================================================================
// Plan Management Tests
// ============================================================================

#[tokio::test]
async fn test_create_plan() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "RefactorPlan".to_string(),
            description: Some("Plan to refactor the codebase.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!plan.id.is_empty());
    assert_eq!(plan.node_type, NodeType::Plan);
    assert_eq!(plan.name, "RefactorPlan");
    assert_eq!(
        plan.description.as_deref(),
        Some("Plan to refactor the codebase.")
    );
}

#[tokio::test]
async fn test_get_plan_by_id() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "MyPlan".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retrieved = get_plan(&addr, &plan.id).await.unwrap();
    assert_eq!(retrieved.id, plan.id);
    assert_eq!(retrieved.name, "MyPlan");
}

#[tokio::test]
async fn test_get_plan_not_found() {
    let (addr, _system) = setup();

    let result = get_plan(&addr, "nonexistent-plan").await;
    assert!(result.is_err(), "Non-existent plan should error");
}

#[tokio::test]
async fn test_get_plan_steps() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "MultiStepPlan".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step1 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step2 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 2".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link steps to plan
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step1.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let steps = get_plan_steps(&addr, &plan.id).await.unwrap();
    assert_eq!(steps.len(), 2);
    let names: Vec<&str> = steps.iter().map(|s| s.name.as_str()).collect();
    assert!(names.contains(&"Step 1"));
    assert!(names.contains(&"Step 2"));
}

#[tokio::test]
async fn test_get_plan_steps_empty() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "EmptyPlan".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let steps = get_plan_steps(&addr, &plan.id).await.unwrap();
    assert!(steps.is_empty(), "Plan with no steps should return empty list");
}

#[tokio::test]
async fn test_get_next_step() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "OrderedPlan".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step1 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "First Step".to_string(),
            properties: Some(HashMap::from([(
                "step_order".to_string(),
                serde_json::json!(1),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step2 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Second Step".to_string(),
            properties: Some(HashMap::from([(
                "step_order".to_string(),
                serde_json::json!(2),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link steps to plan
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step1.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // After step 1 (order=1), next should be step 2 (order=2)
    let next = get_next_step(&addr, &plan.id, 1).await.unwrap();
    assert!(next.is_some(), "Should find a next step");
    assert_eq!(next.unwrap().name, "Second Step");

    // After step 2 (order=2), there should be no next step
    let no_next = get_next_step(&addr, &plan.id, 2).await.unwrap();
    assert!(no_next.is_none(), "Should not find a next step after the last");
}

// ============================================================================
// Execution Management Tests
// ============================================================================

#[tokio::test]
async fn test_start_execution() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_001".to_string(),
            description: Some("First execution.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!execution.id.is_empty());
    assert_eq!(execution.node_type, NodeType::Execution);
    assert_eq!(execution.name, "exec_001");
    assert_eq!(
        execution.properties.get("status").and_then(|v| v.as_str()),
        Some("running"),
        "Execution should start with 'running' status"
    );
    assert!(
        execution.properties.contains_key("start_time"),
        "Execution should have a start_time"
    );
    assert_eq!(execution.version, 1);
}

#[tokio::test]
async fn test_update_execution_status_to_success() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_to_succeed".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Update to success
    update_execution_status(
        &addr,
        &execution.id,
        "success",
        Some("Task completed successfully.".to_string()),
    )
    .await
    .unwrap();

    let updated = get_execution(&addr, &execution.id).await.unwrap();
    assert_eq!(
        updated.properties.get("status").and_then(|v| v.as_str()),
        Some("success")
    );
    assert_eq!(
        updated.properties.get("status_message").and_then(|v| v.as_str()),
        Some("Task completed successfully.")
    );
    assert!(
        updated.properties.contains_key("end_time"),
        "Successful execution should have an end_time"
    );
    assert_eq!(updated.version, 2);
}

#[tokio::test]
async fn test_update_execution_status_to_failed() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_to_fail".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Update to failed
    update_execution_status(
        &addr,
        &execution.id,
        "failed",
        Some("Timeout error.".to_string()),
    )
    .await
    .unwrap();

    let updated = get_execution(&addr, &execution.id).await.unwrap();
    assert_eq!(
        updated.properties.get("status").and_then(|v| v.as_str()),
        Some("failed")
    );
    assert!(
        updated.properties.contains_key("end_time"),
        "Failed execution should have an end_time"
    );
}

#[tokio::test]
async fn test_get_execution_not_found() {
    let (addr, _system) = setup();

    let result = get_execution(&addr, "nonexistent-exec").await;
    assert!(result.is_err(), "Non-existent execution should error");
}

#[tokio::test]
async fn test_get_execution_history() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "BusyAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create two executions
    let exec1 = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let exec2 = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_2".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link both executions to agent
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec1.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec2.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let history = get_execution_history(&addr, &agent.id, 10).await.unwrap();
    assert_eq!(history.len(), 2);
    let names: Vec<&str> = history.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"exec_1"));
    assert!(names.contains(&"exec_2"));
}

#[tokio::test]
async fn test_get_successful_executions() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "SuccessAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create a successful execution
    let exec_success = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "good_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_execution_status(&addr, &exec_success.id, "success", None)
        .await
        .unwrap();

    // Create a failed execution
    let exec_fail = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "bad_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_execution_status(&addr, &exec_fail.id, "failed", None)
        .await
        .unwrap();

    // Link both to agent
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec_success.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec_fail.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let successes = get_successful_executions(&addr, &agent.id, 10).await.unwrap();
    assert_eq!(successes.len(), 1);
    assert_eq!(successes[0].name, "good_exec");
}

#[tokio::test]
async fn test_get_failed_executions() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "FailAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create a successful execution
    let exec_success = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "good_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_execution_status(&addr, &exec_success.id, "success", None)
        .await
        .unwrap();

    // Create a failed execution
    let exec_fail = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "bad_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    update_execution_status(&addr, &exec_fail.id, "failed", None)
        .await
        .unwrap();

    // Link both to agent
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec_success.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec_fail.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let failures = get_failed_executions(&addr, &agent.id, 10).await.unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].name, "bad_exec");
}

// ============================================================================
// Task Result Management Tests
// ============================================================================

#[tokio::test]
async fn test_record_task_result() {
    let (addr, _system) = setup();

    let result = record_task_result(
        &addr,
        NodeInput {
            node_type: NodeType::TaskResult,
            name: "result_001".to_string(),
            description: Some("Task completed with output X.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!result.id.is_empty());
    assert_eq!(result.node_type, NodeType::TaskResult);
    assert_eq!(result.name, "result_001");
    assert_eq!(
        result.description.as_deref(),
        Some("Task completed with output X.")
    );
}

#[tokio::test]
async fn test_get_task_results() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_with_results".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tr1 = record_task_result(
        &addr,
        NodeInput {
            node_type: NodeType::TaskResult,
            name: "result_a".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tr2 = record_task_result(
        &addr,
        NodeInput {
            node_type: NodeType::TaskResult,
            name: "result_b".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link results to execution
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePartOfExecution {
            task_result_id: tr1.id.clone(),
            execution_id: execution.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePartOfExecution {
            task_result_id: tr2.id.clone(),
            execution_id: execution.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let results = get_task_results(&addr, &execution.id).await.unwrap();
    assert_eq!(results.len(), 2);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"result_a"));
    assert!(names.contains(&"result_b"));
}

// ============================================================================
// Artifact Management Tests
// ============================================================================

#[tokio::test]
async fn test_record_artifact() {
    let (addr, _system) = setup();

    let artifact = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "output.json".to_string(),
            subtype: Some("json".to_string()),
            description: Some("Generated output file.".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!artifact.id.is_empty());
    assert_eq!(artifact.node_type, NodeType::Artifact);
    assert_eq!(artifact.name, "output.json");
    assert_eq!(artifact.subtype, Some("json".to_string()));
}

#[tokio::test]
async fn test_get_artifacts() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "artifact_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let art1 = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "file_a.txt".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let art2 = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "file_b.txt".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link artifacts to execution
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateProduced {
            execution_id: execution.id.clone(),
            artifact_id: art1.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateProduced {
            execution_id: execution.id.clone(),
            artifact_id: art2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    let artifacts = get_artifacts(&addr, &execution.id).await.unwrap();
    assert_eq!(artifacts.len(), 2);
    let names: Vec<&str> = artifacts.iter().map(|a| a.name.as_str()).collect();
    assert!(names.contains(&"file_a.txt"));
    assert!(names.contains(&"file_b.txt"));
}

#[tokio::test]
async fn test_get_latest_artifact() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "ArtifactAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create first execution with a log artifact
    let exec1 = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let log_artifact = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "exec1.log".to_string(),
            subtype: Some("log".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec1.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateProduced {
            execution_id: exec1.id.clone(),
            artifact_id: log_artifact.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // Create second execution with a log artifact (should be the latest)
    let exec2 = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "exec_2".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let latest_log = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "exec2.log".to_string(),
            subtype: Some("log".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: exec2.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateProduced {
            execution_id: exec2.id.clone(),
            artifact_id: latest_log.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // Get latest log artifact
    let latest = get_latest_artifact(&addr, &agent.id, "log").await.unwrap();
    assert!(latest.is_some(), "Should find a latest artifact");
    assert_eq!(latest.unwrap().name, "exec2.log");
}

#[tokio::test]
async fn test_get_latest_artifact_no_match() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "NoArtifactAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let latest = get_latest_artifact(&addr, &agent.id, "nonexistent_type").await.unwrap();
    assert!(latest.is_none(), "Should not find any artifact");
}

// ============================================================================
// Error Pattern Management Tests
// ============================================================================

#[tokio::test]
async fn test_record_error() {
    let (addr, _system) = setup();

    let error = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "NullPointerException".to_string(),
            description: Some("Null pointer in payment module.".to_string()),
            properties: Some(HashMap::from([
                ("fingerprint".to_string(), serde_json::json!("fp_001")),
                ("severity".to_string(), serde_json::json!("high")),
            ])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert!(!error.id.is_empty());
    assert_eq!(error.node_type, NodeType::ErrorPattern);
    assert_eq!(error.name, "NullPointerException");
    assert_eq!(
        error.properties.get("fingerprint").and_then(|v| v.as_str()),
        Some("fp_001")
    );
    assert_eq!(
        error.properties.get("severity").and_then(|v| v.as_str()),
        Some("high")
    );
}

#[tokio::test]
async fn test_get_error_by_fingerprint() {
    let (addr, _system) = setup();

    record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "TimeoutError".to_string(),
            properties: Some(HashMap::from([(
                "fingerprint".to_string(),
                serde_json::json!("fp_timeout_123"),
            )])),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let found = get_error_by_fingerprint(&addr, "fp_timeout_123").await.unwrap();
    assert!(found.is_some(), "Should find error by fingerprint");
    assert_eq!(found.unwrap().name, "TimeoutError");
}

#[tokio::test]
async fn test_get_error_by_fingerprint_not_found() {
    let (addr, _system) = setup();

    let found = get_error_by_fingerprint(&addr, "nonexistent_fp").await.unwrap();
    assert!(found.is_none(), "Should not find non-existent fingerprint");
}

#[tokio::test]
async fn test_get_similar_errors_by_tags() {
    let (addr, _system) = setup();

    // Create an error with tags
    let error1 = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "DBConnectionError".to_string(),
            properties: Some(HashMap::from([
                ("fingerprint".to_string(), serde_json::json!("fp_db_1")),
                (
                    "tags".to_string(),
                    serde_json::json!(["database", "connection"]),
                ),
            ])),
            embedding_id: Some("emb_db".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create a similar error (same tags)
    let error2 = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "DBTimeoutError".to_string(),
            properties: Some(HashMap::from([
                ("fingerprint".to_string(), serde_json::json!("fp_db_2")),
                (
                    "tags".to_string(),
                    serde_json::json!(["database", "timeout"]),
                ),
            ])),
            embedding_id: Some("emb_db_2".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Create a different error (no matching tags)
    record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "AuthError".to_string(),
            properties: Some(HashMap::from([
                ("fingerprint".to_string(), serde_json::json!("fp_auth")),
                (
                    "tags".to_string(),
                    serde_json::json!(["auth", "permission"]),
                ),
            ])),
            embedding_id: Some("emb_auth".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Get similar errors to error1 by its embedding_id
    let similar = get_similar_errors(&addr, "emb_db", 10).await.unwrap();
    assert_eq!(similar.len(), 1, "Should find 1 similar error by tag overlap");
    assert_eq!(similar[0].name, "DBTimeoutError");
}

#[tokio::test]
async fn test_link_error_to_fix() {
    let (addr, _system) = setup();

    let error = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "Bug".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "fix_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Link error to fix
    link_error_to_fix(
        &addr,
        &error.id,
        &execution.id,
        HashMap::from([("fix_type".to_string(), serde_json::json!("patch"))]),
    )
    .await
    .unwrap();

    // Verify the relationship was created
    let edges = get_relationships(&addr, &error.id).await.unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].edge_type, RelationshipType::ResolvedBy);
    assert_eq!(edges[0].from_id, error.id);
    assert_eq!(edges[0].to_id, execution.id);
    assert_eq!(
        edges[0].properties.get("fix_type").and_then(|v| v.as_str()),
        Some("patch")
    );
}

// ============================================================================
// Agent Relationship Tests
// ============================================================================

#[tokio::test]
async fn test_create_uses_tool_relationship() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "ToolUserAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let tool = create_tool(
        &addr,
        NodeInput {
            node_type: NodeType::Tool,
            name: "hammer".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateUsesTool {
            agent_id: agent.id.clone(),
            tool_id: tool.id.clone(),
            properties: HashMap::from([("mode".to_string(), serde_json::json!("read"))]),
            reply_to: tx,
        },
    )

    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::UsesTool);
    assert_eq!(edge.from_id, agent.id);
    assert_eq!(edge.to_id, tool.id);
    assert_eq!(
        edge.properties.get("mode").and_then(|v| v.as_str()),
        Some("read")
    );
}

#[tokio::test]
async fn test_create_follows_plan_relationship() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "PlanFollower".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "MasterPlan".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateFollowsPlan {
            agent_id: agent.id.clone(),
            plan_id: plan.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )

    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::FollowsPlan);
    assert_eq!(edge.from_id, agent.id);
    assert_eq!(edge.to_id, plan.id);
}

#[tokio::test]
async fn test_create_contains_step_relationship() {
    let (addr, _system) = setup();

    let plan = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::Plan,
            name: "PlanWithSteps".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step A".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateContainsStep {
            plan_id: plan.id.clone(),
            step_id: step.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::ContainsStep);
    assert_eq!(edge.from_id, plan.id);
    assert_eq!(edge.to_id, step.id);
}

#[tokio::test]
async fn test_create_precedes_relationship() {
    let (addr, _system) = setup();

    let step1 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 1".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step2 = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step 2".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePrecedes {
            from_step_id: step1.id.clone(),
            to_step_id: step2.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::Precedes);
    assert_eq!(edge.from_id, step1.id);
    assert_eq!(edge.to_id, step2.id);
}

#[tokio::test]
async fn test_precedes_cycle_detection() {
    let (addr, _system) = setup();

    let step_a = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step A".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step_b = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step B".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let step_c = create_plan(
        &addr,
        NodeInput {
            node_type: NodeType::PlanStep,
            name: "Step C".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // A -> B -> C
    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePrecedes {
            from_step_id: step_a.id.clone(),
            to_step_id: step_b.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePrecedes {
            from_step_id: step_b.id.clone(),
            to_step_id: step_c.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    // C -> A should create a cycle and be rejected
    let result = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePrecedes {
            from_step_id: step_c.id.clone(),
            to_step_id: step_a.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await;

    assert!(result.is_err(), "Cyclic precedes should be rejected");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("cycle") || err.contains("Acyclic"),
        "Error should mention cycle: {}",
        err
    );

    // Self-loop should also be rejected
    let self_loop = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePrecedes {
            from_step_id: step_a.id.clone(),
            to_step_id: step_a.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await;

    assert!(self_loop.is_err(), "Self-loop precedes should be rejected");
}

#[tokio::test]
async fn test_create_produced_relationship() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "producing_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let artifact = record_artifact(
        &addr,
        NodeInput {
            node_type: NodeType::Artifact,
            name: "output.txt".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateProduced {
            execution_id: execution.id.clone(),
            artifact_id: artifact.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::Produced);
    assert_eq!(edge.from_id, execution.id);
    assert_eq!(edge.to_id, artifact.id);
}

#[tokio::test]
async fn test_create_encountered_error_relationship() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "failing_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let error = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "RuntimeError".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateEncounteredError {
            execution_id: execution.id.clone(),
            error_id: error.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::EncounteredError);
    assert_eq!(edge.from_id, execution.id);
    assert_eq!(edge.to_id, error.id);
}

#[tokio::test]
async fn test_create_resolved_by_relationship() {
    let (addr, _system) = setup();

    let error = record_error(
        &addr,
        NodeInput {
            node_type: NodeType::ErrorPattern,
            name: "FixableError".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "fixing_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateResolvedBy {
            error_id: error.id.clone(),
            execution_id: execution.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::ResolvedBy);
    assert_eq!(edge.from_id, error.id);
    assert_eq!(edge.to_id, execution.id);
}

#[tokio::test]
async fn test_create_part_of_execution_relationship() {
    let (addr, _system) = setup();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "parent_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let task_result = record_task_result(
        &addr,
        NodeInput {
            node_type: NodeType::TaskResult,
            name: "sub_result".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreatePartOfExecution {
            task_result_id: task_result.id.clone(),
            execution_id: execution.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::PartOfExecution);
    assert_eq!(edge.from_id, task_result.id);
    assert_eq!(edge.to_id, execution.id);
}

#[tokio::test]
async fn test_create_executed_by_relationship() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "WorkerAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "worker_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateExecutedBy {
            execution_id: execution.id.clone(),
            agent_id: agent.id.clone(),
            properties: HashMap::new(),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::ExecutedBy);
    assert_eq!(edge.from_id, execution.id);
    assert_eq!(edge.to_id, agent.id);
}

#[tokio::test]
async fn test_create_learned_from_relationship() {
    let (addr, _system) = setup();

    let agent = create_agent(
        &addr,
        NodeInput {
            node_type: NodeType::Agent,
            name: "LearningAgent".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let execution = start_execution(
        &addr,
        NodeInput {
            node_type: NodeType::Execution,
            name: "learning_exec".to_string(),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let edge = create_agent_relationship(
        &addr,
        |tx| MemoryGraphMessage::CreateLearnedFrom {
            agent_id: agent.id.clone(),
            execution_id: execution.id.clone(),
            properties: HashMap::from([("lesson".to_string(), serde_json::json!("don't use recursion"))]),
            reply_to: tx,
        },
    )
    .await
    .unwrap();

    assert_eq!(edge.edge_type, RelationshipType::LearnedFrom);
    assert_eq!(edge.from_id, agent.id);
    assert_eq!(edge.to_id, execution.id);
    assert_eq!(
        edge.properties.get("lesson").and_then(|v| v.as_str()),
        Some("don't use recursion")
    );
}



