# Spire Rust Core

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![Crates.io](https://img.shields.io/badge/crate-0.1.0-orange)](Cargo.toml)

The Rust MCP server core for the Spire VS Code extension. This is the main processing engine — a native binary that handles code analysis, knowledge graph operations, semantic search, and LLM orchestration.

It communicates with the VS Code extension via the **Model Context Protocol (MCP)** over stdio, using JSON-RPC 2.0 messages.

---

## Architecture

```
core/
├── src/
│   ├── main.rs              # Entry point: initializes embedder, graph DB, MCP server
│   ├── lib.rs               # Crate root — re-exports all public modules
│   ├── mcp/                 # MCP protocol layer (rust-mcp-sdk)
│   │   ├── server.rs        # SpireMcpHandler — implements ServerHandler trait
│   │   ├── tools.rs         # Tool definitions & call handlers
│   │   └── client.rs        # External MCP server connection manager
│   ├── actors/              # Actor system (tonari-actor)
│   │   ├── coordinator.rs   # Workflow orchestrator
│   │   ├── memory_graph.rs  # Knowledge graph actor (sole data store)
│   │   ├── progress.rs      # Progress broadcaster
│   │   └── llm.rs           # LLM gateway client
│   ├── embedder/            # Text embedding pipeline
│   │   └── candle_embedder.rs  # Candle-based all-MiniLM-L6-v2 embedder
│   ├── graph/               # Graph database wrapper
│   │   └── mod.rs           # GraphDb — SeleneDB wrapper with WAL persistence
│   └── models/              # Shared data structures
│       ├── analysis.rs      # Code analysis types
│       ├── embedding.rs     # Embedding types & Embedder trait
│       ├── graph.rs         # Graph query/result types
│       └── memory_graph.rs  # Core graph node/edge/memory types
└── tests/
    ├── embedder_test.rs     # Embedding integration tests (requires model download)
    └── integration_test.rs  # Actor system & MCP server tests
```

---

## Components

### MCP Layer (`mcp/`)

The MCP layer handles protocol communication with the VS Code extension:

- **`server.rs`** — `SpireMcpHandler` implements the `ServerHandler` trait from `rust-mcp-sdk`. Handles `list_tools` and `call_tool` requests.
- **`tools.rs`** — Defines four tools and their handlers:
  - `explain_code` — Explain a code snippet
  - `search_codebase` — Regex or semantic search
  - `analyze_dependencies` — Dependency graph analysis
  - `get_code_metrics` — Code quality metrics
- **`client.rs`** — `McpClientManager` manages connections to external MCP servers (e.g., filesystem, GitHub, Postgres). Currently a stub ready for integration.

### Actor System (`actors/`)

Built on `tonari-actor`, the system uses four actors communicating via message passing:

| Actor | File | Role |
|-------|------|------|
| `CoordinatorActor` | `coordinator.rs` | Top-level orchestrator; receives user requests, delegates to other actors |
| `MemoryGraphActor` | `memory_graph.rs` | **Sole data store** — owns graph nodes, edges, and vector embeddings directly |
| `ProgressActor` | `progress.rs` | Broadcasts progress updates via `tokio::sync::broadcast` |
| `LlmActor` | `llm.rs` | LLM gateway client (stub — echoes prompt) |

The `MemoryGraphActor` enforces schema constraints:
- **Unique `(type, name)`** per node
- **Referential integrity** for relationships (both endpoints must exist)
- **Acyclic `depends_on`** relationships (cycle detection via DFS)

### Embedder (`embedder/`)

Uses **Candle** (a minimalist ML framework) to run the `sentence-transformers/all-MiniLM-L6-v2` model locally:

- **384-dimensional** L2-normalized embedding vectors
- Model weights (~85 MB) downloaded from Hugging Face Hub on first run, cached at `~/.cache/huggingface/`
- **Metal GPU acceleration** on Apple Silicon (opt-in via `SPIRE_USE_METAL=1`; CPU by default)
- Async `Embedder` trait for text → vector generation, spawned via `tokio::spawn`

### Graph Database (`graph/`)

A high-level wrapper around **SeleneDB** — a lock-free, WAL-persisted graph database:

- **`GraphDb`** struct provides a simplified API for node/edge CRUD, traversal, and vector search
- **Write-Ahead Log** persistence (path configurable via `SPIRE_WAL_PATH`)
- **Vector indexes** for semantic search over node embeddings
- **Compaction** support to reclaim space from tombstones

### Data Models (`models/`)

| Module | Key Types |
|--------|-----------|
| `memory_graph.rs` | `GraphNode`, `GraphEdge`, `NodeType`, `RelationshipType`, `MemoryEntry`, `SearchOptions`, `TraversalResult`, `ProjectSnapshot` |
| `graph.rs` | `GraphQuery`, `GraphQueryType`, `GraphResult` |
| `analysis.rs` | `CodeAnalysis`, `CodeAnalysisRequest`, `ComplexityScore`, `SymbolInfo`, `SearchResult` |
| `embedding.rs` | `Embedding`, `Embedder` trait |

---

## Prerequisites

- **Rust** 1.75+ (stable)
- **Cargo** (included with Rust)

On first run, the embedding model (~85 MB) will be downloaded to `~/.cache/huggingface/`.

---

## Build & Test

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run all tests (excluding model download tests)
cargo test

# Run embedding tests (requires model download, ~85 MB)
cargo test -- --ignored

# Run with logging
RUST_LOG=spire_rust=debug cargo run

# Build documentation
cargo doc --open
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SPIRE_WAL_PATH` | `spire-graph.wal` | Path to the Write-Ahead Log file for graph persistence |
| `SPIRE_USE_METAL` | (unset) | Set to `1` to enable Metal GPU acceleration for embeddings |
| `RUST_LOG` | `info,spire_rust=debug` | Logging level filter |

### Release Profile

The `Cargo.toml` release profile is optimized for production:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
```

---

## Key Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `rust-mcp-sdk` | 0.10 | MCP protocol implementation |
| `tonari-actor` | 0.12 | Actor framework for message passing |
| `candle-core` | 0.11 | ML inference engine (with Metal support) |
| `candle-transformers` | 0.11 | BERT model implementation |
| `hf-hub` | 1.0.0-rc.1 | Hugging Face Hub client for model downloads |
| `tokenizers` | 0.21 | Text tokenization |
| `selene-db-core` | 1.4.0 | Graph database core |
| `selene-db-graph` | 1.4.0 | Graph operations |
| `selene-db-gql` | 1.4.0 | Graph query language |
| `selene-db-persist` | 1.4.0 | WAL persistence |
| `tokio` | 1 | Async runtime |
| `serde` / `serde_json` | 1 | Serialization |

---

## Development Notes

### Adding a New Tool

1. Define the tool in `mcp/tools.rs` using the `make_input_schema` helper
2. Add a handler function in the same file
3. Register the tool in `get_tools()` and the handler in `handle_tool_call()`
4. Wire the handler to the coordinator actor in `mcp/server.rs`

### Adding a New Actor

1. Define the message enum and actor struct in a new file under `actors/`
2. Implement the `Actor` trait from `tonari-actor`
3. Register the actor in `actors/mod.rs`
4. Spawn it in `main.rs` and pass its address to the coordinator

### Testing Conventions

- Unit tests are co-located with source code (e.g., `candle_embedder.rs` has inline tests)
- Integration tests live in `tests/`
- Tests that require model downloads are marked `#[ignore]` and run with `cargo test -- --ignored`
- The `TestEmbedder` in `tests/integration_test.rs` provides a no-op embedder for testing

---

## License

GNU GPLv3 — see [LICENSE](../LICENSE) for details.
