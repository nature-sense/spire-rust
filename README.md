# Spire Rust

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![VS Code](https://img.shields.io/badge/vscode-1.90%2B-blueviolet)](https://code.visualstudio.com)
[![License](https://img.shields.io/badge/license-GPLv3-blue)](LICENSE)

**Spire Rust** is a VS Code extension powered by a native Rust MCP (Model Context Protocol) server. It provides intelligent code analysis, knowledge graph traversal, and semantic search capabilities directly in the editor — all running locally with no cloud dependency.

The Rust core handles all heavy lifting: embedding generation via Candle (all-MiniLM-L6-v2), graph storage via SeleneDB, and an actor-based orchestration system. The TypeScript extension is a thin UI shell that spawns the Rust binary and communicates over stdio.

---

## Features

- **🧠 Explain Code** — Select any code snippet and get an AI-powered explanation
- **🔍 Search Codebase** — Semantic or regex-based search across your project
- **📊 Analyze Code** — Static analysis with complexity scoring and symbol extraction
- **🔗 Knowledge Graph** — Persistent graph database tracking project entities, decisions, and relationships
- **📝 Memory & Context** — Recall past conversations and project context across sessions
- **🛠️ MCP Tools** — Exposes tools via the Model Context Protocol for integration with any MCP client

---

## Architecture

```
spire-rust/
├── extension/          # TypeScript VS Code Extension (thin UI shell)
│   ├── src/
│   │   ├── extension.ts     # Lifecycle: activate/deactivate, spawns Rust process
│   │   ├── mcp-client.ts    # JSON-RPC MCP client over stdio
│   │   ├── ui/
│   │   │   ├── chat-panel.ts   # Webview chat interface
│   │   │   └── status-bar.ts   # Status bar indicator
│   │   └── commands.ts      # VS Code command registrations
│   └── media/               # Webview assets
│
├── core/               # Rust MCP Server (native binary)
│   ├── src/
│   │   ├── main.rs          # Entry: actor system + MCP server
│   │   ├── mcp/             # MCP protocol (rust-mcp-sdk)
│   │   │   ├── server.rs    # MCP message handler
│   │   │   ├── tools.rs     # Tool definitions
│   │   │   └── client.rs    # External MCP server connection manager
│   │   ├── actors/          # tonari-actor based system
│   │   │   ├── coordinator.rs   # Workflow orchestrator
│   │   │   ├── memory_graph.rs  # Knowledge graph actor (sole data store)
│   │   │   ├── progress.rs      # Progress broadcaster
│   │   │   └── llm.rs           # LLM gateway client
│   │   ├── embedder/        # Text embedding (Candle + all-MiniLM-L6-v2)
│   │   ├── graph/           # SeleneDB graph database wrapper
│   │   └── models/          # Shared data structures
│   └── tests/               # Integration tests
│
└── doc/                # Reference documentation
    └── messages-and-types.md  # Actor message & data type reference
```

### Actor System

The Rust core uses a `tonari-actor` based system with four actors:

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

| Actor | Role |
|-------|------|
| `CoordinatorActor` | Top-level orchestrator; receives user requests, delegates to other actors |
| `MemoryGraphActor` | Sole data store — owns graph nodes, edges, and vector embeddings directly |
| `ProgressActor` | Broadcasts progress updates via `tokio::sync::broadcast` |
| `LlmActor` | LLM gateway client (stub — ready for provider integration) |

### MCP Tools

| Tool | Description | Required Params |
|------|-------------|-----------------|
| `explain_code` | Explain a code snippet | `code: string` |
| `search_codebase` | Regex or semantic search | `query: string` |
| `analyze_dependencies` | Dependency graph analysis | `path: string` |
| `get_code_metrics` | Code quality metrics | `path: string` |

---

## Prerequisites

- **Rust** 1.75+ (stable)
- **Node.js** 20+
- **pnpm** (recommended) or npm
- **VS Code** 1.90+

On first run, the embedding model (~85 MB) will be downloaded to `~/.cache/huggingface/`.

---

## Quick Start

```bash
# Clone the repository
git clone https://github.com/naturesense/spire-rust.git
cd spire-rust

# Install dependencies
pnpm install

# Build everything (Rust + TypeScript)
pnpm run build

# Development (build + launch VS Code debug session)
pnpm run dev

# Run tests
pnpm run test

# Package as .vsix
pnpm run package
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `SPIRE_WAL_PATH` | `spire-graph.wal` | Path to the Write-Ahead Log file for graph persistence |
| `SPIRE_USE_METAL` | (unset) | Set to `1` to enable Metal GPU acceleration for embeddings (may fail on unsupported ops) |

### VS Code Commands

| Command | Keybinding (macOS) | Keybinding (Windows/Linux) |
|---------|-------------------|---------------------------|
| `Spire: Explain Code` | `Cmd+Shift+E` | `Ctrl+Shift+E` |
| `Spire: Open Chat` | `Cmd+Shift+I` | `Ctrl+Shift+I` |
| `Spire: Search Codebase` | — | — |
| `Spire: Analyze Code` | — | — |

---

## Project Structure

| Directory | Description |
|-----------|-------------|
| `extension/` | VS Code extension (TypeScript) |
| `core/` | Rust MCP server binary |
| `doc/` | Reference documentation |
| `.vscode/` | VS Code debug & task configurations |

---

## Development

### Building Individually

```bash
# Build only the Rust core
cd core && cargo build --release

# Build only the TypeScript extension
cd extension && npm run compile
```

### Testing

```bash
# Run all tests
pnpm run test

# Run Rust tests only (excluding model download tests)
cd core && cargo test

# Run embedding tests (requires model download, ~85 MB)
cd core && cargo test -- --ignored
```

### Debugging

The `.vscode/launch.json` and `.vscode/tasks.json` files provide pre-configured debug and build tasks for VS Code.

---

## Project Status

Spire Rust is in **early development** (v0.1.0). The architecture is in place, but several features are stubs awaiting implementation:

- [ ] **LLM integration** — The `LlmActor` currently echoes prompts; needs provider integration (OpenAI, Anthropic, local models)
- [ ] **Code analysis** — Tool handlers return placeholder responses; need actual parsing and analysis
- [ ] **Actor system wiring** — The coordinator actor is defined but not yet spawned in `main.rs`
- [ ] **Vector search** — SeleneDB vector index integration is partially implemented
- [ ] **External MCP clients** — `McpClientManager` is a stub ready for third-party server connections

---

## License

GNU GPLv3 — see [LICENSE](LICENSE) for details.

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, and check the [issue tracker](https://github.com/naturesense/spire-rust/issues) for open issues.

---

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the project history.
