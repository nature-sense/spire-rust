# Actor System — Actors & Message Types

> **Last updated:** 2026-07-09

This document catalogs every actor in the system, its message enum variants, and how they connect.

---

## Framework (`spire-core/src/framework/`)

### `Actor` trait (`actor.rs`)

```rust
#[async_trait]
pub trait Actor: Send + 'static {
    type Message: Send + 'static;
    async fn handle(&mut self, msg: Self::Message);
    fn spawn(mut self, mut rx: mpsc::Receiver<Self::Message>) -> JoinHandle<()>;
}
```

Every actor implements this trait. The default `spawn()` loops on `rx.recv()` and calls `handle()` for each message.

### Core message types (`messages.rs`)

| Type | Fields | Purpose |
|------|--------|---------|
| `ListToolsMessage` | `response_tx: Responder<Vec<ToolInfo>>` | Request to list all registered tools |
| `ToolMessage` | `tool: String, args: Value, response_tx: Responder<Value>` | Generic tool invocation (used by tool actors) |
| `ToolInfo` | `name, description, input_schema` | Metadata describing a registered tool |
| `ActorError` | `ToolNotFound, Io, Serialization, ChannelClosed, Internal` | Typed errors for the actor system |

### `ActorSystem` (`system.rs`)

Thread-safe registry using `DashMap<String, Box<dyn Any>>` mapping actor names to `mpsc::Sender<M>`.

```rust
impl ActorSystem {
    pub fn new() -> Self;
    pub fn spawn<A: Actor>(&self, name: &str, actor: A) -> mpsc::Sender<A::Message>;
    pub fn get<M: Send + 'static>(&self, name: &str) -> Option<mpsc::Sender<M>>;
}
```

---

## Core Actors (`spire-core/src/actors/`)

### 1. CoordinatorActor (`coordinator.rs`)

**Purpose:** Main workflow orchestrator. Receives user requests and delegates to other actors.

**State:** Holds `mpsc::Sender` for chat, tools, mcp_client, llm, progress, and system channels.

```
CoordinatorMessage:
  ├── HandleRequest {
  │     method: String,
  │     params: Value,
  │     response_tx: oneshot::Sender<Value>
  │   }
  └── Shutdown
```

**Connections:**
- Forwards tool calls to `ToolsActor`
- Forwards chat messages to `ChatActor`
- Forwards MCP operations to `McpClientActor`
- Forwards LLM requests to `LlmActor`
- Forwards progress updates to `ProgressActor`
- Forwards system operations to `SystemActor`

---

### 2. ChatActor (`chat.rs`)

**Purpose:** Manages chat dialogs and message history.

**State:** `HashMap<String, Dialog>` — dialog ID → dialog state.

```
ChatMessage:
  ├── GetActive {
  │     reply_to: oneshot::Sender<Option<Dialog>>
  │   }
  ├── Create {
  │     title: Option<String>,
  │     reply_to: oneshot::Sender<Dialog>
  │   }
  ├── SendMessage {
  │     dialog_id: String,
  │     content: String,
  │     role: String,
  │     reply_to: oneshot::Sender<Result<Message, ActorError>>
  │   }
  ├── GetHistory {
  │     dialog_id: String,
  │     reply_to: oneshot::Sender<Option<Vec<Message>>>
  │   }
  ├── ListDialogs {
  │     reply_to: oneshot::Sender<Vec<DialogSummary>>
  │   }
  └── DeleteDialog {
        dialog_id: String,
        reply_to: oneshot::Sender<bool>
      }
```

---

### 3. ToolsActor (`tools.rs`)

**Purpose:** Manages tool registration and execution. Wraps both embedded tools and VS Code tools.

**State:** `Vec<Box<dyn Tool>>` — registered tool implementations.

```
ToolsMessage:
  ├── ListTools {
  │     reply_to: oneshot::Sender<Vec<ToolInfo>>
  │   }
  └── CallTool {
        tool: String,
        args: Value,
        reply_to: oneshot::Sender<Result<Value, ActorError>>
      }
```

---

### 4. McpClientActor (`mcp_client.rs`)

**Purpose:** Wraps `McpClientManager` — manages connections to external MCP servers (e.g. filesystem, git, etc.).

**State:** `McpClientManager`

```
McpClientMessage:
  ├── LoadConfig {
  │     reply_to: oneshot::Sender<Result<Option<PathBuf>, ActorError>>
  │   }
  ├── ConnectAll {
  │     reply_to: oneshot::Sender<Result<(), ActorError>>
  │   }
  ├── Connect {
  │     server_name: String,
  │     reply_to: oneshot::Sender<Result<(), ActorError>>
  │   }
  ├── DisconnectAll {
  │     reply_to: oneshot::Sender<Result<(), ActorError>>
  │   }
  ├── Disconnect {
  │     server_name: String,
  │     reply_to: oneshot::Sender<Result<(), ActorError>>
  │   }
  ├── GetTools {
  │     server_name: String,
  │     reply_to: oneshot::Sender<Option<Vec<Tool>>>
  │   }
  ├── ConnectedServers {
  │     reply_to: oneshot::Sender<Vec<String>>
  │   }
  └── CallTool {
        server_name: String,
        tool_name: String,
        arguments: Option<Map<String, Value>>,
        reply_to: oneshot::Sender<Result<CallToolResult, ActorError>>
      }
```

---

### 5. LlmActor (`llm.rs`)

**Purpose:** LLM gateway. Currently a stub that echoes back the prompt.

**State:** Stateless.

```
LlmMessage:
  ├── Complete {
  │     prompt: String,
  │     reply_to: oneshot::Sender<Result<String, ActorError>>
  │   }
  └── Stream {
        prompt: String,
        reply_to: oneshot::Sender<Result<Receiver<String>, ActorError>>
      }
```

---

### 6. ProgressActor (`progress.rs`)

**Purpose:** Broadcasts progress updates to subscribers via `tokio::sync::broadcast`.

**State:** `broadcast::Sender<ProgressUpdate>` (buffer: 256)

```
ProgressMessage:
  ├── Publish(ProgressUpdate {
  │     task_id: String,
  │     message: String,
  │     percent: f64,
  │     status: ProgressStatus  // Running | Completed | Failed
  │   })
  └── Subscribe {
        reply_to: oneshot::Sender<Result<Receiver<ProgressUpdate>, ActorError>>
      }
```

---

### 7. SystemActor (`system.rs`)

**Purpose:** Handles system-level operations (shutdown, health checks).

**State:** None.

```
SystemMessage:
  ├── Shutdown {
  │     reply_to: oneshot::Sender<Result<(), ActorError>>
  │   }
  └── Health {
        reply_to: oneshot::Sender<Value>
      }
```

---

### 8. VSCode Tools (`vscode_tools.rs`)

**Purpose:** Defines the VS Code tool definitions that are registered with the tools actor. These are tool stubs that the VS Code extension implements on the other side of the JSON-RPC bridge.

**State:** Static definitions.

```rust
pub fn vscode_tool_definitions() -> Vec<ToolInfo>;
```

---

## Architecture Diagram

```
                          ┌──────────────┐
                          │  JSON-RPC    │  stdin/stdout
                          │  Transport   │  ←→ VS Code Extension
                          └──────┬───────┘
                                 │
                    ┌────────────▼────────────┐
                    │     CoordinatorActor     │  orchestrates workflows
                    │  (request router)       │
                    └──┬───┬───┬───┬───┬──────┘
                       │   │   │   │   │
              ┌────────┘   │   │   │   └──────────┐
              ▼            ▼   │   ▼               ▼
       ┌──────────┐  ┌────────┐│┌──────────┐ ┌──────────┐
       │ Chat     │  │ Tools  │││   LLM    │ │ Progress │
       │ Actor    │  │ Actor  │││  Actor   │ │  Actor   │
       └──────────┘  └────────┘│└──────────┘ └──────────┘
                               │
                        ┌──────▼──────┐
                        │ McpClient   │
                        │   Actor     │──→ External MCP Servers
                        │             │    (mcp-git, mcp-search,
                        └─────────────┘     mcp-process, etc.)
```

---

## Spawning Pattern

```rust
let system = ActorSystem::new();

// Spawn actors — each returns an mpsc::Sender for its message type
let (chat_tx, _handle) = system.spawn(ChatActor::new());
let (tools_tx, _handle) = system.spawn(ToolsActor::new());
let (mcp_client_tx, _handle) = system.spawn(McpClientActor::new());
let (llm_tx, _handle) = system.spawn(LlmActor::new(LlmConfig::default()));
let (progress_tx, _handle) = system.spawn(ProgressActor::new());
let (system_tx, _handle) = system.spawn(SystemActor::new());

// Coordinator needs the senders of other actors
let (coord_tx, _handle) = system.spawn(
    CoordinatorActor::new(
        chat_tx, tools_tx, mcp_client_tx,
        llm_tx, progress_tx, system_tx,
        transport_arc.clone(),
    ),
);
```

---

## MCP Configuration

The `McpConfig` system provides configuration for external MCP server connections.

### Config file location

| Priority | Source | Path |
|----------|--------|------|
| 1 | `SPIRE_MCP_CONFIG` env var | Arbitrary path |
| 2 | Default | `~/.spire/mcp-config.json` |

### Config structure

```json
{
  "external_servers": [
    {
      "name": "filesystem",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": null
    }
  ]
}
```

### Types

| Type | Fields | Purpose |
|------|--------|---------|
| `McpConfig` | `external_servers` | Top-level config |
| `ExternalServerConfig` | `name, command, args, env` | External MCP server definition |
| `ConfigError` | `Io`, `Parse` | Error types for config loading |

### Loading flow

1. `main.rs` calls `McpConfig::load()` at startup
2. If `~/.spire/mcp-config.json` doesn't exist, no external servers are configured
3. External server configs are stored for later connection by `McpClientActor`

---

## Error Handling

All actor responses use `ActorError`:

| Variant | Meaning |
|---------|---------|
| `ToolNotFound(String)` | Requested tool name is not registered |
| `Io(io::Error)` | Filesystem or I/O operation failed |
| `Serialization(serde_json::Error)` | JSON serialization/deserialization failed |
| `ChannelClosed` | The oneshot receiver was dropped before sending |
| `Internal(String)` | Any other internal error |
