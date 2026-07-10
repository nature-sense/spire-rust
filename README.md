# Spire Rust

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![VS Code](https://img.shields.io/badge/vscode-1.85%2B-blueviolet)](https://code.visualstudio.com)
[![License](https://img.shields.io/badge/license-GPLv3-blue)](LICENSE)

**Spire Rust** is an AI coding assistant for VS Code. It consists of two parts:

- **`spire-core/`** — The Rust core engine (actor-based orchestration, LLM integration, knowledge graph, MCP client management). Runs as a subprocess of the extension, communicating via JSON-RPC 2.0 over stdin/stdout.
- **`spire-extension/`** — The VS Code extension (TypeScript). Thin UI shell that spawns the Rust binary and provides the editor interface.

---

## Features

- **💬 Chat Interface** — Conversational AI assistant with streaming responses
- **🧠 Explain Code** — Select any code snippet and get an AI-powered explanation
- **🔍 Search Codebase** — Semantic or regex-based search across your project
- **📊 Analyze Code** — Static analysis with complexity scoring and symbol extraction
- **🔗 Knowledge Graph** — Persistent graph database tracking project entities, decisions, and relationships
- **📝 Memory & Context** — Recall past conversations and project context across sessions
- **🛠️ MCP Tools** — Connects to external MCP servers (git, search, process) for extended capabilities
- **⚙️ Config Editor** — Manage Spire settings from a dedicated WebView

---

## Architecture

```
spire-rust/
├── spire-core/            # Rust core engine (subprocess)
│   ├── src/
│   │   ├── main.rs            # Entry point: stdio transport + actor system
│   │   ├── lib.rs             # Crate root
│   │   ├── framework/         # Actor framework (actor, system, messages)
│   │   ├── actors/            # Actor implementations
│   │   │   ├── coordinator.rs # Workflow orchestrator
│   │   │   ├── chat.rs        # Chat session management
│   │   │   ├── llm.rs         # LLM gateway client
│   │   │   ├── tools.rs       # Tool registry & execution
│   │   │   ├── vscode_tools.rs# VS Code tool bridge
│   │   │   ├── mcp_client.rs  # External MCP server client
│   │   │   ├── progress.rs    # Progress broadcaster
│   │   │   └── system.rs      # System management
│   │   ├── mcp/               # MCP protocol layer
│   │   │   └── client.rs      # MCP client connection manager
│   │   └── transport/         # stdio transport (JSON-RPC 2.0)
│   │       └── stdio.rs       # Line-delimited JSON over stdin/stdout
│   └── tests/                 # Integration tests
│
├── spire-extension/       # VS Code extension (TypeScript)
│   ├── src/
│   │   ├── extension.ts       # Lifecycle: activate/deactivate
│   │   ├── client/            # MCP client & environment client
│   │   ├── server/            # JSON-RPC server (router + handlers)
│   │   │   ├── transport.ts   # stdio transport management
│   │   │   ├── router.ts      # Request routing
│   │   │   └── handlers/      # Tool handlers (workspace, git, editor, etc.)
│   │   ├── model/             # Type definitions & message schemas
│   │   ├── util/              # Utilities (logger)
│   │   └── webview/           # Chat & config WebView UI
│   └── test/                  # Integration tests
│
├── mcp/                   # External MCP server implementations
│   ├── mcp-git/              # Git operations MCP server
│   ├── mcp-process/          # Process management MCP server
│   └── mcp-search/           # Code search MCP server
│
└── doc/                   # Reference documentation
    ├── extension-core-interface.md  # JSON-RPC protocol between extension & core
    ├── spire-actor-framework.md     # Actor system design
    ├── json-rpc-protocol.md         # JSON-RPC 2.0 message reference
    └── ...
```

### Communication Flow

```
┌──────────────────────┐     JSON-RPC 2.0      ┌──────────────────────┐
│  spire-extension     │◄───── stdin/stdout ───▶│  spire-core          │
│  (VS Code Extension) │                        │  (Rust subprocess)   │
│                      │                        │                      │
│  ┌────────────────┐  │                        │  ┌────────────────┐  │
│  │ Server/Router  │──┼────────────────────────┼─▶│ StdioTransport │  │
│  │ (tool handlers)│  │                        │  └────────┬───────┘  │
│  └────────────────┘  │                        │           │          │
│                      │                        │  ┌────────▼───────┐  │
│  ┌────────────────┐  │                        │  │ Actor System   │  │
│  │ Client/Transport│  │                        │  │ (coordinator,  │  │
│  │ (stdio mgmt)   │  │                        │  │  chat, llm,    │  │
│  └────────────────┘  │                        │  │  tools, ...)   │  │
│                      │                        │  └────────────────┘  │
│  ┌────────────────┐  │                        │                      │
│  │ WebView (Chat) │  │                        │  ┌────────────────┐  │
│  └────────────────┘  │                        │  │ MCP Clients    │──┼──▶ External MCP Servers
│                      │                        │  │ (git, search,  │  │    (git, search, process)
└──────────────────────┘                        │  │  process)      │  │
                                                 │  └────────────────┘  │
                                                 └──────────────────────┘
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

# Build the Rust core
cd spire-core && cargo build && cd ..

# Build the extension
cd spire-extension && npm run build && cd ..

# Or use VS Code: Run Extension (F5) with the pre-configured launch config
```

---

## Project Structure

| Directory | Description |
|-----------|-------------|
| `spire-core/` | Rust core engine (actor system, LLM, MCP client) |
| `spire-extension/` | VS Code extension (TypeScript) |
| `mcp/` | External MCP server implementations |
| `doc/` | Reference documentation |
| `.vscode/` | VS Code debug & task configurations |

---

## Development

### Building

```bash
# Build the Rust core
cd spire-core && cargo build

# Build the extension
cd spire-extension && npm run build

# Run Rust tests
cd spire-core && cargo test

# Run extension tests
cd spire-extension && npm test
```

### Debugging

The `.vscode/launch.json` and `.vscode/tasks.json` files provide pre-configured debug and build tasks for VS Code. Use **Run Extension** (F5) to launch a development VS Code window with the extension loaded.

---

## License

GNU GPLv3 — see [LICENSE](LICENSE) for details.

---

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, and check the [issue tracker](https://github.com/naturesense/spire-rust/issues) for open issues.
