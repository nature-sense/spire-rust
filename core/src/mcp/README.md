# Spire MCP — Model Context Protocol Layer

[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--11--28-blueviolet)](https://modelcontextprotocol.io)

The MCP layer implements the [Model Context Protocol](https://modelcontextprotocol.io) — the communication bridge between the VS Code extension and the Rust backend. It handles tool registration, request dispatching, and external MCP server connections.

---

## Architecture

```
mcp/
├── mod.rs       # Re-exports: SpireMcpHandler, McpClientManager, get_tools
├── server.rs    # SpireMcpHandler — implements ServerHandler trait
├── tools.rs     # Tool definitions & call handlers (4 tools)
└── client.rs    # McpClientManager — external MCP server connections
```

### Communication Flow

```
┌─────────────────────┐         JSON-RPC 2.0          ┌─────────────────────┐
│  VS Code Extension  │  ──────────────────────────►  │  Rust MCP Server    │
│  (TypeScript)       │  ◄──────────────────────────  │  (Native Binary)    │
│                     │       over stdio (stdin/stdout)│                     │
│  mcp-client.ts      │                                │  SpireMcpHandler    │
└─────────────────────┘                                └─────────────────────┘
```

---

## Server Handler (`server.rs`)

`SpireMcpHandler` implements the `ServerHandler` trait from `rust-mcp-sdk`:

```rust
pub struct SpireMcpHandler {
    coordinator: Option<CoordinatorActor>,  // TODO: wire up actor system
}
```

### Protocol Methods

| Method | Description |
|--------|-------------|
| `handle_list_tools_request` | Returns the list of available tools from `tools.rs` |
| `handle_call_tool_request` | Dispatches to the appropriate handler in `tools.rs` |

The handler is wrapped via `.to_mcp_server_handler()` and served over stdio transport:

```rust
let transport = StdioTransport::new(options)?;
let server = server_runtime::create_server(McpServerOptions {
    transport,
    handler,
    server_details,
    task_store: None,
    client_task_store: None,
    message_observer: None,
});
server.start().await?;
```

### Server Identity

```rust
InitializeResult {
    server_info: Implementation {
        name: "spire-rust",
        version: env!("CARGO_PKG_VERSION"),
        title: "Spire Rust MCP Server",
        description: "A Rust-powered MCP server for code analysis and knowledge graph operations",
    },
    capabilities: ServerCapabilities {
        tools: Some(ServerCapabilitiesTools { list_changed: None }),
    },
    protocol_version: ProtocolVersion::V2025_11_28,
}
```

---

## Tool Definitions (`tools.rs`)

Four tools are exposed via MCP:

### Tool: `explain_code`

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `code` | `string` | ✅ | The code snippet or file path to explain |
| `language` | `string` | ❌ | Programming language (auto-detected if omitted) |

### Tool: `search_codebase`

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `query` | `string` | ✅ | The search query (regex or natural language) |
| `mode` | `string` | ❌ | Search mode: `"regex"` or `"semantic"` |
| `path` | `string` | ❌ | Optional path to scope the search |

### Tool: `analyze_dependencies`

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | `string` | ✅ | The file or module path to analyze |
| `depth` | `integer` | ❌ | Maximum depth for dependency traversal (default: 1) |

### Tool: `get_code_metrics`

| Property | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | `string` | ✅ | The file or directory path to analyze |
| `metrics` | `array[string]` | ❌ | Specific metrics to compute (e.g., `complexity`, `loc`) |

### Adding a New Tool

1. Define the tool using the `make_input_schema` helper
2. Add a handler function (e.g., `handle_my_tool`)
3. Register in `get_tools()` and `handle_tool_call()`

```rust
fn my_tool() -> Tool {
    Tool {
        name: "my_tool".into(),
        description: Some("Does something useful".into()),
        input_schema: make_input_schema(
            vec![("param", serde_json::json!({"type": "string", "description": "..."}))],
            vec!["param"],
        ),
        ..Default::default()
    }
}
```

---

## External Client (`client.rs`)

`McpClientManager` manages connections to external MCP servers — a stub ready for integration with third-party services.

```rust
pub struct McpServerConfig {
    pub name: String,       // e.g., "filesystem", "github", "postgres"
    pub command: String,    // e.g., "npx", "uvx"
    pub args: Vec<String>,  // e.g., ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
    pub env: Option<HashMap<String, String>>,
}
```

### Planned External Servers

| Server | Purpose |
|--------|---------|
| **Filesystem** | Read/write files, search directory trees |
| **GitHub** | Repository management, PRs, issues |
| **Postgres** | Database querying and schema inspection |
| **Puppeteer** | Browser automation and web scraping |
| **Slack** | Messaging and channel management |
| **Memory** | Persistent knowledge graph |
| **Brave Search** | Web and local search |
| **Sequential Thinking** | Multi-step reasoning |

---

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `rust-mcp-sdk` | 0.10 | MCP protocol implementation (server + client) |
| `serde` / `serde_json` | 1 | JSON serialization for MCP messages |
| `tokio` | 1 | Async runtime for transport |

---

## Related

- [Actors README](../actors/README.md) — How the MCP handler connects to the coordinator actor
- [Extension README](../../../extension/README.md) — The TypeScript MCP client
- [Core README](../../README.md) — Project overview
