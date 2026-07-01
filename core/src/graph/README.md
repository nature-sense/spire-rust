# Spire Graph — SeleneDB Database Wrapper

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![SeleneDB](https://img.shields.io/badge/selene--db-1.4.0-green)](https://crates.io/crates/selene-db-graph)

The graph module provides a high-level wrapper around [SeleneDB](https://github.com/naturesense/selene-db), a lock-free, WAL-persisted graph database. It exposes a simplified API for node/edge CRUD, traversal, and vector search — the persistence backbone of the Spire knowledge graph.

---

## Architecture

```
graph/
└── mod.rs  # GraphDb — the sole public type
```

### GraphDb (`mod.rs`)

`GraphDb` wraps SeleneDB's `SharedGraph` and provides a clean, idiomatic Rust API:

```rust
pub struct GraphDb {
    shared: Arc<SharedGraph>,  // SeleneDB shared graph (lock-free reads)
    graph_id: GraphId,         // Unique graph identifier
}
```

Key design decisions:

- **Lock-free reads**: SeleneDB uses `ArcSwap` for snapshot isolation — readers never block writers
- **Serialized writes**: All mutations funnel through a single committer thread, preserving strict serializability
- **WAL persistence**: Optional Write-Ahead Log for crash recovery (configurable path)
- **Clone-friendly**: `GraphDb` is `Clone + Send + Sync` via `Arc` — share across threads safely

---

## API Reference

### Construction

| Method | Description |
|--------|-------------|
| `new_in_memory()` | Create an in-memory graph (no persistence). Ideal for testing. |
| `new_with_wal(path)` | Create a WAL-backed graph. Recovers from existing WAL on open. |

### Node Operations

| Method | Returns | Description |
|--------|---------|-------------|
| `create_node(labels, properties)` | `Result<NodeId>` | Create a node with labels and properties |
| `get_node(node_id)` | `Option<GraphNode>` | Get a node by its SeleneDB `NodeId` |
| `delete_node(node_id)` | `Result<()>` | Delete a node (tombstone-based) |
| `node_count()` | `usize` | Number of live nodes |

### Edge Operations

| Method | Returns | Description |
|--------|---------|-------------|
| `create_edge(subject, predicate, object, properties)` | `Result<EdgeId>` | Create a directed edge (subject → predicate → object) |
| `get_edge(edge_id)` | `Option<GraphEdge>` | Get an edge by its SeleneDB `EdgeId` |
| `delete_edge(edge_id)` | `Result<()>` | Delete an edge |
| `edge_count()` | `usize` | Number of edges |

### Traversal & Query

| Method | Returns | Description |
|--------|---------|-------------|
| `outgoing_edges(node_id)` | `Vec<GraphEdge>` | All edges where the node is the source |
| `incoming_edges(node_id)` | `Vec<GraphEdge>` | All edges where the node is the target |
| `nodes_with_label(label)` | `Vec<GraphNode>` | All nodes with a given label |
| `edges_with_label(label)` | `Vec<GraphEdge>` | All edges with a given label |

### Vector Search

| Method | Returns | Description |
|--------|---------|-------------|
| `create_vector_index(label, property, dimensions, metric)` | `Result<()>` | Create a vector index for semantic search |
| `vector_search(label, property, query_vector, limit)` | `Result<Vec<VectorSearchHit>>` | KNN search over indexed embeddings |

### Maintenance

| Method | Returns | Description |
|--------|---------|-------------|
| `compact()` | `Result<()>` | Reclaim space from tombstones via compaction |
| `rebuild_vector_indexes()` | `Result<()>` | Rebuild all vector indexes from scratch |

---

## Usage

### Basic CRUD

```rust
use spire_rust::graph::GraphDb;
use selene_core::value::Value;

let db = GraphDb::new_in_memory()?;

// Create nodes
let alice = db.create_node(
    vec!["Person".into()],
    vec![("name".into(), Value::from("Alice"))],
)?;

let bob = db.create_node(
    vec!["Person".into()],
    vec![("name".into(), Value::from("Bob"))],
)?;

// Create a relationship
let knows = db.create_edge(
    alice,
    "knows",
    bob,
    vec![("since".into(), Value::from(2024))],
)?;

// Query
let alice_node = db.get_node(alice).unwrap();
println!("Found: {}", alice_node.name);

let edges = db.outgoing_edges(alice);
println!("Alice has {} outgoing edges", edges.len());
```

### Vector Search

```rust
use selene_core::vector::VectorMetric;

// Create a vector index on Document nodes' embedding property
db.create_vector_index("Document", "embedding", 384, VectorMetric::Cosine)?;

// Search
let query = vec![0.1_f32; 384]; // Your embedding vector
let results = db.vector_search("Document", "embedding", query, 10)?;
for hit in results {
    println!("Node {}: score = {}", hit.node_id, hit.score);
}
```

---

## SeleneDB Internals

SeleneDB is a property graph database with the following characteristics:

| Feature | Detail |
|---------|--------|
| **Storage** | Dense row stores with tombstones for deletion |
| **Concurrency** | Lock-free reads (`ArcSwap`), single-writer committer |
| **Persistence** | Write-Ahead Log (WAL) with configurable sync policy |
| **Indexes** | Property indexes, text indexes, vector indexes (HNSW) |
| **IDs** | Compact `u64` NodeId/EdgeId with monotonic allocation |
| **Compaction** | Reclaims tombstone space, renumbers rows under stable IDs |
| **Schema** | Optional closed-type catalog (ISO 39075:2024) |

### WAL Persistence

The WAL is configured with `SyncPolicy::OnFlushOnly` — the committer thread owns all fsync calls, ensuring strict serializability without redundant syncs. The WAL path is configurable via the `SPIRE_WAL_PATH` environment variable.

---

## Testing

```rust
#[test]
fn test_in_memory_graph() {
    let db = GraphDb::new_in_memory().unwrap();
    let node = db.create_node(
        vec!["Test".into()],
        vec![("key".into(), Value::from("value"))],
    ).unwrap();
    assert!(db.get_node(node).is_some());
}
```

```bash
# Run graph tests
cargo test

# Run with WAL persistence tests
SPIRE_WAL_PATH=/tmp/test.wal cargo test
```

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `selene-db-core` | 1.4.0 | Core types (NodeId, EdgeId, Value, PropertyMap) |
| `selene-db-graph` | 1.4.0 | Graph operations (SharedGraph, WriteTxn, Mutator) |
| `selene-db-persist` | 1.4.0 | WAL persistence (WalConfig, WalWriter) |
| `selene-db-gql` | 1.4.0 | Graph query language (future use) |

---

## Related

- [Actors README](../actors/README.md) — How `MemoryGraphActor` wraps `GraphDb`
- [Models README](../models/README.md) — `GraphNode` and `GraphEdge` data types
- [Core README](../../README.md) — Project overview
