# Spire Actors — Message-Passing Orchestration System

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![tonari-actor](https://img.shields.io/badge/tonari--actor-0.12-purple)](https://crates.io/crates/tonari-actor)

The actor system is the brain of Spire — four actors communicate via message passing to orchestrate code analysis, knowledge graph operations, progress reporting, and LLM interactions. Built on [`tonari-actor`](https://crates.io/crates/tonari-actor), a lightweight Rust actor framework.

---

## Architecture

```
actors/
├── mod.rs           # Re-exports all actors and their message types
├── coordinator.rs   # Workflow orchestrator
├── memory_graph.rs  # Knowledge graph actor (sole data store)
├── progress.rs      # Progress broadcaster
└── llm.rs           # LLM gateway client
```

### Message Flow

```
                    ┌──────────────────┐
                    │  CoordinatorActor │──→ ProgressActor (broadcast progress)
                    │  (orchestrator)   │──→ LlmActor (LLM calls)
                    └──────┬───────────┘
                           │
                           ▼
                    MemoryGraphActor
                    (sole data store:
                     nodes, edges,
                     embeddings)
                           │
                      Embedder (trait)
```

| Actor | File | Role |
|-------|------|------|
| `CoordinatorActor` | `coordinator.rs` | Top-level orchestrator; receives user requests, delegates to other actors |
| `MemoryGraphActor` | `memory_graph.rs` | **Sole data store** — owns graph nodes, edges, and vector embeddings directly |
| `ProgressActor` | `progress.rs` | Broadcasts progress updates via `tokio::sync::broadcast` |
| `LlmActor` | `llm.rs` | LLM gateway client (stub — ready for provider integration) |

---

## CoordinatorActor (`coordinator.rs`)

The top-level orchestrator. Receives user-facing requests and coordinates the other actors.

### Messages

```rust
pub enum CoordinatorMessage {
    ExplainCode {
        code: String,
        language: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    SearchCodebase {
        query: String,
        max_results: usize,
        reply_to: tokio::sync::oneshot::Sender<Result<Vec<(String, f64)>>>,
    },
    AnalyzeCode {
        request: CodeAnalysisRequest,
        reply_to: tokio::sync::oneshot::Sender<Result<CodeAnalysis>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `ExplainCode` | `Result<String>` | Explain a code snippet via LLM |
| `SearchCodebase` | `Result<Vec<(String, f64)>>` | Semantic search; returns `(node_id, score)` pairs |
| `AnalyzeCode` | `Result<CodeAnalysis>` | Static analysis of code |

### Workflow

For each request, the coordinator:
1. Publishes a progress update via `ProgressActor`
2. Delegates to the appropriate actor (LLM, MemoryGraph)
3. Returns the result via the `oneshot` channel

---

## MemoryGraphActor (`memory_graph.rs`)

The **sole data store** for the system. This actor owns all graph nodes, edges, vector embeddings, and memory entries directly — no separate sub-actors. It wraps `GraphDb` (SeleneDB) for persistence.

### Messages (14 variants)

```rust
pub enum MemoryGraphMessage {
    // ── Node Operations ──
    GetNode { id: String, reply_to: ... },
    QueryNodes { filter: NodeFilter, reply_to: ... },
    StoreNode { node: NodeInput, reply_to: ... },
    UpdateNode { id: String, updates: NodeUpdate, reply_to: ... },
    DeleteNode { id: String, reply_to: ... },

    // ── Relationship Operations ──
    CreateRelationship { rel: RelationshipInput, reply_to: ... },
    GetRelationships { node_id: String, reply_to: ... },
    DeleteRelationship { id: String, reply_to: ... },

    // ── Traversal ──
    Traverse { start_node_id: String, options: TraversalOptions, reply_to: ... },

    // ── Context & Memory ──
    GetProjectContext { reply_to: ... },
    SearchContext { query: String, options: Option<SearchOptions>, reply_to: ... },
    AddMemory { text: String, metadata: Option<MemoryMetadata>, reply_to: ... },
    Recall { query: String, limit: Option<usize>, reply_to: ... },

    // ── Maintenance ──
    Sync { reply_to: ... },
}
```

### Schema Constraints

The actor enforces the following invariants:

| Constraint | Enforcement |
|-----------|-------------|
| **Unique `(type, name)`** | No two nodes can share the same type and name |
| **Referential integrity** | Both endpoints must exist before creating a relationship |
| **Acyclic `depends_on`** | DFS cycle detection prevents circular dependencies |
| **Valid relationship types** | Only registered `RelationshipType` variants are accepted |

### ID Mapping

The external API uses UUID-based `String` IDs (for compatibility with the TypeScript extension), while SeleneDB uses compact `u64` IDs (`NodeId`/`EdgeId`). The actor maintains a bidirectional mapping between the two.

### Embedding

When a node is stored or updated with a description, the actor spawns an async embedding task via `tokio::spawn`:

```rust
let embedder = self.embedder.clone();
let text = format!("{}: {}", node.name, desc);
tokio::spawn(async move {
    match embedder.embed(&text).await {
        Ok(embedding) => { /* store in graph */ }
        Err(e) => tracing::warn!("Embedding failed: {}", e),
    }
});
```

---

## ProgressActor (`progress.rs`)

A simple broadcast actor for progress updates.

### Messages

```rust
pub enum ProgressMessage {
    Publish(ProgressUpdate),
    Subscribe {
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::broadcast::Receiver<ProgressUpdate>>,
    },
}
```

### Types

```rust
pub struct ProgressUpdate {
    pub task_id: String,
    pub message: String,
    pub percent: f64,
    pub status: ProgressStatus,
}

pub enum ProgressStatus {
    Running,
    Completed,
    Failed,
}
```

Uses a `tokio::sync::broadcast` channel (capacity: 256) for fan-out to multiple subscribers.

---

## LlmActor (`llm.rs`)

LLM gateway client. Currently a **stub** — ready for provider integration.

### Messages

```rust
pub enum LlmMessage {
    Complete {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<Result<String>>,
    },
    Stream {
        prompt: String,
        reply_to: tokio::sync::oneshot::Sender<tokio::sync::mpsc::Receiver<String>>,
    },
}
```

| Variant | Reply Type | Description |
|---------|-----------|-------------|
| `Complete` | `Result<String>` | Single response (stub: echoes first 100 chars) |
| `Stream` | `mpsc::Receiver<String>` | Streaming token-by-token (stub: sends one token) |

### Planned Integrations

- OpenAI / Anthropic API
- Local models via llama.cpp or similar
- Ollama for local LLM serving

---

## Wiring Diagram

In `main.rs`, the actors are spawned and connected:

```
main.rs
  │
  ├── spawns MemoryGraphActor  → Addr<MemoryGraphMessage>
  │     (receives Arc<dyn Embedder> + Arc<GraphDb>)
  ├── spawns LlmActor          → Addr<LlmMessage>
  ├── spawns ProgressActor     → Addr<ProgressMessage>
  └── spawns CoordinatorActor  → Addr<CoordinatorMessage>
        (receives memory_graph, llm, progress addrs)
```

---

## Testing

```bash
# Run actor system tests
cargo test

# Run with logging
RUST_LOG=spire_rust=debug cargo test

# Run specific test
cargo test test_memory_graph_store_node
```

The `TestEmbedder` in `tests/integration_test.rs` provides a no-op embedder for testing without model downloads.

---

## Related

- [Graph README](../graph/README.md) — The `GraphDb` wrapper used by `MemoryGraphActor`
- [Embedder README](../embedder/README.md) — The embedding pipeline used for vector search
- [Models README](../models/README.md) — Data types used across all actors
- [doc/messages-and-types.md](../../../doc/messages-and-types.md) — Complete message reference
- [Core README](../../README.md) — Project overview
