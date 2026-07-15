# Spire Rust

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![VS Code](https://img.shields.io/badge/vscode-1.85%2B-blueviolet)](https://code.visualstudio.com)
[![License](https://img.shields.io/badge/license-GPLv3-blue)](LICENSE)

**Spire Rust** is an AI coding assistant for VS Code. It consists of two parts:

- **`rust/spire-core/`** — The Rust core engine (actor-based orchestration, LLM integration, knowledge graph, MCP client management). Runs as a subprocess of the extension, communicating via JSON-RPC 2.0 over a TCP loopback socket.
- **`ts/spire-extension/`** — The VS Code extension (TypeScript). Thin UI shell that spawns the Rust binary and provides the editor interface.

---

## Features

- **💬 Chat Interface** — Conversational AI assistant with streaming responses
- **🧠 Explain Code** — Select any code snippet and get an AI-powered explanation
- **🔍 Search Codebase** — Semantic or regex-based search across your project
- **📊 Analyze Code** — Static analysis with complexity scoring and symbol extraction
- **🔗 Knowledge Graph** — Persistent graph database tracking project entities, decisions, and relationships
- **📝 Memory & Context** — Recall past conversations and project context across sessions
- **🛠️ MCP Tools** — Connects to external MCP servers (git, search, process, terminal, filesystem) for extended capabilities
- **⚙️ Config Editor** — Manage Spire settings from a dedicated WebView

---

## Architecture

```
spire-rust/
├── rust/                       # All Rust crates
│   ├── spire-core/                 # Rust core engine (subprocess)
│   │   ├── src/
│   │   │   ├── main.rs                # Entry point: TCP socket + actor system
│   │   │   ├── lib.rs                 # Crate root
│   │   │   ├── framework/             # Actor framework (actor, system, messages)
│   │   │   ├── actors/                # Actor implementations
│   │   │   ├── mcp/                   # MCP protocol layer
│   │   │   ├── transport/             # TCP socket transport (JSON-RPC 2.0)
│   │   │   ├── graph/                 # Graph database wrapper (SeleneDB)
│   │   │   ├── embedder/              # Text embedding (Candle)
│   │   │   └── models/                # Shared data structures
│   │   └── tests/                     # Integration & actor tests
│   ├── mcp/                       # External MCP server implementations
│   │   ├── mcp-git/                  # Git operations MCP server
│   │   ├── mcp-process/              # Process management MCP server
│   │   ├── mcp-search/               # Code search MCP server
│   │   ├── mcp-terminal/             # Terminal management MCP server
│   │   └── mcp-filesystem/           # Filesystem operations MCP server
│   └── tools/                       # Development tools
│       └── project-analyzer/         # Project structure analyzer
│
├── ts/                        # All TypeScript/Node.js projects
│   └── spire-extension/           # VS Code extension (TypeScript)
│       ├── src/
│       │   ├── extension.ts           # Lifecycle: activate/deactivate
│       │   ├── client/                # Bidirectional client & environment client
│       │   ├── server/                # JSON-RPC server (router + handlers)
│       │   ├── model/                 # Type definitions & message schemas
│       │   ├── util/                  # Utilities (logger)
│       │   └── webview/               # Chat & config WebView UI
│       └── test/                      # Integration tests
│
├── scripts/                    # Build & packaging scripts
├── doc/                        # Reference documentation
└── .vscode/                    # VS Code debug & task configurations
```

### Communication Flow

```
┌──────────────────────────┐   JSON-RPC 2.0    ┌──────────────────────────┐
│  spire-extension         │◄─── TCP socket ───▶│  spire-core             │
│  (VS Code Extension)     │   127.0.0.1:<port> │  (Rust subprocess)      │
│                          │                    │                          │
│  ┌────────────────────┐  │                    │  ┌────────────────────┐  │
│  │ BidirectionalClient│──┼────────────────────┼─▶│ SocketTransport    │  │
│  │ (req/resp routing) │  │                    │  └────────┬───────────┘  │
│  └────────────────────┘  │                    │           │              │
│                          │                    │  ┌────────▼───────────┐  │
│  ┌────────────────────┐  │                    │  │ Actor System       │  │
│  │ Local Router       │  │                    │  │ (coordinator,      │  │
│  │ (workspace, editor,│  │                    │  │  chat, llm,        │  │
│  │  git, terminal,    │  │                    │  │  tools, memory_    │  │
│  │  diagnostics, ...) │  │                    │  │  graph, project_   │  │
│  └────────────────────┘  │                    │  │  sync, mcp_client, │  │
│                          │                    │  │  system, ...)      │  │
│  ┌────────────────────┐  │                    │  └────────────────────┘  │
│  │ WebView (Chat)     │  │                    │                          │
│  └────────────────────┘  │                    │  ┌────────────────────┐  │
│                          │                    │  │ MCP Clients        │──┼──▶ External MCP Servers
│  ┌────────────────────┐  │                    │  │ (git, search,      │  │    (git, search, process,
│  │ Status Bar         │  │                    │  │  process, terminal,│  │     terminal, filesystem)
│  └────────────────────┘  │                    │  │  filesystem)       │  │
└──────────────────────────┘                    │  └────────────────────┘  │
                                                 └──────────────────────────┘
```

---

## Prerequisites

- **Rust** 1.75+ (stable)
- **Node.js** 18+
- **pnpm** (recommended) or npm
- **VS Code** 1.85+

---

## Quick Start

```bash
# Clone the repository
git clone https://github.com/naturesense/spire-rust.git
cd spire-rust

# Install dependencies
pnpm install

# Build everything (Rust workspace + extension)
pnpm run build

# Or use VS Code: Run Extension (F5) with the pre-configured launch config
```

---

## Project Structure

| Directory | Description |
|-----------|-------------|
| `rust/spire-core/` | Rust core engine (actor system, LLM, MCP client) |
| `rust/mcp/` | External MCP server implementations |
| `rust/tools/` | Development tools (project-analyzer, etc.) |
| `ts/spire-extension/` | VS Code extension (TypeScript) |
| `doc/` | Reference documentation |
| `.vscode/` | VS Code debug & task configurations |

---

## Development

### Building

```bash
# Build the entire Rust workspace (core + MCP servers + tools)
cd rust && cargo build --workspace

# Build the extension
cd ts/spire-extension && npm run build

# Or build everything from the root
pnpm run build

# Run all Rust tests
cd rust && cargo test --workspace

# Run extension tests
cd ts/spire-extension && npm test

# Run all tests from the root
pnpm run test
```

### Project Analyzer

The `project-analyzer` tool scans a project directory and produces a structured analysis (languages, build tools, entry points, directory structure, sub-projects) — useful for giving an LLM semantic understanding of a project.

```bash
# Via the shell wrapper (recommended)
./scripts/analyze.sh . --format pretty

# Via pnpm (if pnpm is configured)
pnpm run analyze -- . --format pretty

# Directly from the rust directory
cd rust && cargo run -p project-analyzer -- . --format pretty

# JSON output (for programmatic use)
./scripts/analyze.sh /path/to/project --format json

# Skip .gitignore (include all files)
./scripts/analyze.sh . --no-ignore --format pretty
```

Output includes:
- **Project type** (rust_workspace, node_package, vscode_extension, python_project, etc.)
- **Languages** with file counts and estimated line counts
- **Build tools** with config files
- **Entry points** (main.rs, extension.ts, package.json scripts, etc.)
- **Directory structure** with classified directories (source_code, documentation, tests, etc.)
- **Key files** with their roles (changelog, license, CI configs, etc.)
- **Recursive sub-project analysis** (Cargo workspace members, pnpm workspace packages)
- **Human-readable summary**

### Debugging

The `.vscode/launch.json` and `.vscode/tasks.json` files provide pre-configured debug and build tasks for VS Code. Use **Run Extension** (F5) to launch a development VS Code window with the extension loaded.

---

## License

GNU GPLv3 — see [LICENSE](LICENSE) for details.

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, and check the [issue tracker](https://github.com/naturesense/spire-rust/issues) for open issues.
