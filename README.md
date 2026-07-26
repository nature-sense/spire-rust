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
- **🔗 Knowledge Graph** — Persistent graph database (SeleneDB) tracking project entities, decisions, and relationships
- **📝 Memory & Context** — Recall past conversations and project context across sessions
- **🛠️ MCP Tools** — Connects to external MCP servers (git, search, process, terminal, filesystem) for extended capabilities
- **🔨 Build-Fix Loop** — Automatic build → error detection → fix → rebuild cycle
- **📋 Plan Mode** — Deliberate multi-step execution plans with approve/pause/retry/skip controls
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
│   │   │   ├── actors/                # 20+ actor implementations
│   │   │   ├── mcp/                   # MCP protocol client
│   │   │   ├── transport/             # TCP socket transport (JSON-RPC 2.0)
│   │   │   ├── graph/                 # Graph database wrapper (SeleneDB)
│   │   │   ├── embedder/              # Text embedding (Candle/DeepSeek)
│   │   │   └── models/                # Shared data structures
│   │   └── tests/                     # Integration & actor tests
│   └── mcp/                       # External MCP server implementations
│       ├── mcp-git/                  # Git operations MCP server
│       ├── mcp-process/              # Process management MCP server
│       ├── mcp-search/               # Code search MCP server
│       ├── mcp-terminal/             # Terminal management MCP server
│       ├── mcp-filesystem/           # Filesystem operations MCP server
│       ├── mcp-cargo/                # Cargo build MCP server
│       └── mcp-node/                 # Node.js build MCP server
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
├── config/                     # Intent, MCP, and tool capability configs
├── doc/                        # Reference documentation
└── .vscode/                    # VS Code debug & task configurations
```

### Actor System

20 actors communicate via `tokio::sync::mpsc` channels, all spawned by the singleton `ActorSystem`:

| Actor | Role |
|-------|------|
| **CoordinatorActor** | Top-level orchestrator; receives user requests, routes to intent router or LLM |
| **IntentRouterActor** | Matches user queries to registered intents (build, fix, plan, etc.) |
| **PromptHandlerActor** | Builds LLM prompts with context injection (graph, system prompt, tools) |
| **ChatActor** | Manages chat dialogs and message history |
| **LlmActor** | LLM gateway — sends prompts to DeepSeek API, returns streaming responses |
| **MemoryGraphActor** | Sole data store — owns GraphDb (SeleneDB) for nodes, edges, and embeddings |
| **ToolOrchestrator** | Executes tools and tool chains with step context (template resolution) |
| **ToolRouterActor** | Routes `project/*`, `build/*`, `test/*` calls to the right actor or MCP |
| **BuildOrchestrator** | Manages the build-fix loop lifecycle (build → fix → rebuild) |
| **ErrorAnalyzer** | Matches build errors to fix strategies via graph lookup |
| **PlanOrchestrator** (NEW) | Creates, stores, and executes multi-step plans with approve/reject flow |
| **ProjectBuildActor** | Per-system build execution via MCP tools |
| **ProjectTestActor** | Test execution via MCP tools |
| **ProjectLintActor** | Lint execution via MCP tools |
| **ProjectInstallActor** | Package installation via MCP tools |
| **ProjectAnalyzerActor** | Semantic project analysis for LLM context |
| **ProjectQueryActor** | Structured project queries for LLM context |
| **ProjectSyncActor** | Three-phase project structure sync |
| **SystemPromptActor** | Caches system prompt prefix (DeepSeek prompt caching) |
| **McpClientActor** | Manages external MCP server connections |
| **ProgressActor** | Broadcasts progress updates via `tokio::sync::broadcast` |
| **SystemActor** | System state machine, startup phase chain |

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
│  ┌────────────────────┐  │                    │  │ CoordinatorActor   │  │
│  │ Local Router       │  │                    │  │ (orchestrator)     │──┼──▶ IntentRouter → ...
│  │ (workspace, editor,│  │                    │  │                    │  │
│  │  git, terminal,    │  │                    │  │  ┌─plan_orch──────┐│  │
│  │  diagnostics, ...) │  │                    │  │  │ PlanOrchestrator││  │
│  └────────────────────┘  │                    │  │  └────────────────┘│  │
│                          │                    │  └────────────────────┘  │
│  ┌────────────────────┐  │                    │                          │
│  │ WebView (Chat)     │  │                    │  ┌────────────────────┐  │
│  └────────────────────┘  │                    │  │ MCP Clients        │──┼──▶ External MCP Servers
│                          │                    │  │ (cargo, git,       │  │    (git, search, process,
│  ┌────────────────────┐  │                    │  │  search, process,  │  │     terminal, filesystem)
│  │ Status Bar         │  │                    │  │  terminal, node,   │  │
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

# Build the Rust workspace
cd rust && cargo build --workspace

# Build the extension
cd ts/spire-extension && npm install && npm run build

# Or use VS Code: Run Extension (F5) with the pre-configured launch config
```

---

## Development

### Building

```bash
# Build the entire Rust workspace (core + MCP servers)
cd rust && cargo build --workspace

# Build the extension
cd ts/spire-extension && npm run build

# Run all Rust tests
cd rust && cargo test --workspace

# Run extension tests
cd ts/spire-extension && npm test
```

### Intent System

The intent router in `config/intents.json` defines 11 registered intents:

| Intent | Handler | Priority | Example triggers |
|--------|---------|----------|-----------------|
| `build` | BuildOrchestrator | 10 | "build", "compile", "cargo build" |
| `plan` | PlanOrchestrator | 8 | "plan how to", "create a plan", "make a plan for" |
| `fix-build-error` | PlanOrchestrator | 9 | "fix build", "build failed", "compilation error" |
| `test` | BuildOrchestrator | 8 | "test", "run tests", "cargo test" |
| `lint` | BuildOrchestrator | 5 | "lint", "clippy", "fmt" |
| `check` | BuildOrchestrator | 6 | "check", "verify", "cargo check" |
| `run` | ToolOrchestrator | 7 | "run", "execute", "start" |
| `analyze-project` | ProjectAnalyzer | 6 | "analyze", "explain project" |
| `add-dependency` | BuildOrchestrator | 4 | "add dep", "cargo add", "npm install" |
| `update-dependencies` | BuildOrchestrator | 4 | "update", "upgrade" |
| `clean` | BuildOrchestrator | 3 | "clean", "cargo clean" |

### Plan Mode

The PlanOrchestrator supports approve/pause/retry/skip flows:

```
User: "plan how to fix the build"
  → PlanOrchestrator creates a plan via LLM
  → Plan stored as Plan + PlanStep nodes in graph
  → Plan-list widget pushed to chat
  → User approves → steps execute sequentially via ToolOrchestrator
  → Real-time widget updates show progress
```

### Debugging

The `.vscode/launch.json` and `.vscode/tasks.json` files provide pre-configured debug and build tasks for VS Code. Use **Run Extension** (F5) to launch a development VS Code window with the extension loaded.

---

## License

GNU GPLv3 — see [LICENSE](LICENSE) for details.

---

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines, and check the [issue tracker](https://github.com/naturesense/spire-rust/issues) for open issues.