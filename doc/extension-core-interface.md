# Extension–Core Interface

This document describes the communication protocol between the VS Code extension (`ts/spire-extension/`) and the Rust core binary (`rust/spire-core/`). The extension spawns the Rust binary as a child process and communicates over stdin/stdout using JSON-RPC 2.0.

---

## Table of Contents

1. [Overview](#overview)
2. [Transport Layer](#transport-layer)
3. [Protocol](#protocol)
4. [Tool Catalog](#tool-catalog)
5. [Notification Schema](#notification-schema)
6. [Lifecycle](#lifecycle)
7. [Error Handling](#error-handling)
8. [Sequence Diagrams](#sequence-diagrams)
9. [Architecture Diagram](#architecture-diagram)

---

## Overview

```
┌─────────────────────────────────────────────────────┐
│              VS Code Extension (TypeScript)          │
│                                                      │
│  ┌──────────┐  ┌──────────┐  ┌──────────────────┐   │
│  │ Chat     │  │ Config   │  │ Extension Host    │   │
│  │ WebView  │  │ WebView  │  │ (activate/deact.) │   │
│  └────┬─────┘  └────┬─────┘  └────────┬─────────┘   │
│       │              │                 │             │
│  ┌────▼──────────────▼─────────────────▼──────────┐  │
│  │              Service Layer                      │  │
│  │  ┌──────────────────┐  ┌──────────────────┐    │  │
│  │  │   ChatService    │  │   ConfigService  │    │  │
│  │  │  (notifications) │  │  (tool calls)    │    │  │
│  │  └────────┬─────────┘  └────────┬─────────┘    │  │
│  │           │                     │               │  │
│  │  ┌────────▼─────────────────────▼──────────┐   │  │
│  │  │           McpClient                      │   │  │
│  │  │  JSON-RPC 2.0 over stdin/stdout          │   │  │
│  │  │  Auto-reconnect, timeouts, notifications │   │  │
│  │  └────────────────────┬─────────────────────┘   │  │
│  └───────────────────────┼─────────────────────────┘  │
│                          │                            │
│                    stdin/stdout                        │
│                    (child_process.spawn)               │
│                          │                            │
│  ┌───────────────────────┼─────────────────────────┐  │
│  │           Rust Core Binary                      │  │
│  │              │                                  │  │
│  │  ┌───────────▼──────────────┐                   │  │
│  │  │  StdioTransport          │                   │  │
│  │  │  (JSON-RPC 2.0 parser)   │                   │  │
│  │  └───────────┬──────────────┘                   │  │
│  │              │                                  │  │
│  │  ┌───────────▼──────────────┐                   │  │
│  │  │  Actor System            │                   │  │
│  │  │  (Coordinator, Chat,     │                   │  │
│  │  │   LLM, Tools, Progress)  │                   │  │
│  │  └──────────────────────────┘                   │  │
│  └─────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────┘
```

The boundary between TypeScript and Rust is the **stdio transport**. The extension writes JSON-RPC 2.0 messages to the Rust process's stdin and reads responses/notifications from its stdout. Stderr is used for logging and diagnostics only.

---

## Transport Layer

### Process Spawning (TypeScript side)

The transport layer in `spire-extension/src/server/transport.ts` spawns the Rust binary:

```typescript
this.process = child_process.spawn(resolvedPath, [], {
  stdio: ['pipe', 'pipe', 'pipe'],  // stdin, stdout, stderr
  env: process.env,
});
```

- **stdin** (`pipe`) — Extension writes JSON-RPC messages to the Rust process
- **stdout** (`pipe`) — Rust writes JSON-RPC responses and notifications (line-delimited)
- **stderr** (`pipe`) — Rust writes log/diagnostic output (displayed in VS Code error notifications)

### Line-Delimited JSON

Messages are newline-delimited. Each JSON object is written as a single line terminated by `\n`. The Rust `StdioTransport` reads lines from stdin; the TypeScript transport uses Node.js `readline` to read lines from stdout.

### Path Resolution

The extension resolves the core binary path in this order:

1. **Absolute path** — used as-is
2. **Extension directory** — checks `spire-extension/bin/` relative to the extension install path
3. **PATH** — uses `which` to find the binary in the system PATH

### Reconnection

If the Rust process exits or crashes, the transport attempts to reconnect with exponential backoff:

| Attempt | Delay |
|---------|-------|
| 1 | 1,000 ms |
| 2 | 2,000 ms |
| 3 | 4,000 ms |
| Max | 10,000 ms |

After 3 failed attempts, reconnection stops. All pending requests are rejected with `"MCP connection lost"`.

---

## Protocol

### JSON-RPC 2.0

All messages conform to [JSON-RPC 2.0](https://www.jsonrpc.org/specification).

#### Request (Extension → Core)

```typescript
interface McpRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;       // Always "tools/call"
  params: {
    name: string;       // Tool name (e.g. "chat/stream", "config/get")
    arguments: Record<string, any>;
  };
}
```

#### Response (Core → Extension)

```typescript
interface McpResponse {
  jsonrpc: '2.0';
  id: number;           // Matches the request id
  result?: any;         // Present on success
  error?: {
    code: number;
    message: string;
    data?: any;
  };
}
```

#### Notification (Extension → Core)

Fire-and-forget messages with no response:

```typescript
interface McpNotification {
  jsonrpc: '2.0';
  method: string;       // e.g. "chat/send"
  params?: any;
}
```

#### Notification (Core → Extension)

Server-sent events with no request id:

```typescript
interface McpNotification {
  jsonrpc: '2.0';
  method: string;       // e.g. "chat/chunk", "agent/progress"
  params?: any;
}
```

### Method Dispatch

The extension uses two calling patterns:

| Pattern | Method | Description |
|---------|--------|-------------|
| `callTool()` | `"tools/call"` | Standard request/response. Sends a request with `id`, receives a response with matching `id`. Used for most operations. |
| `sendNotification()` | Custom method name | Fire-and-forget. No response expected. Used for `chat/send`. |

---

## Tool Catalog

### `chat/send` (Notification)

Send a chat message without waiting for a streaming response.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "method": "chat/send",
  "params": {
    "message": "Explain this code",
    "session_id": "sess_1712345678_a1b2c3d",
    "context": {
      "filePath": "/path/to/file.rs",
      "selection": "fn main() {}",
      "projectRoot": "/path/to/project"
    }
  }
}
```

**Response:** None (fire-and-forget). The core sends `chat/chunk`, `agent/progress`, `agent/complete`, and `agent/error` notifications in response.

---

### `chat/stream` (Tool Call)

Send a chat message and receive a streaming response.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": {
    "name": "chat/stream",
    "arguments": {
      "message": "Explain this code",
      "session_id": "sess_1712345678_a1b2c3d",
      "context": {
        "filePath": "/path/to/file.rs",
        "selection": "fn main() {}",
        "projectRoot": "/path/to/project"
      }
    }
  }
}
```

**Response:** The tool call returns immediately. Streaming content is delivered via `chat/chunk` notifications. The final `agent/complete` notification signals the end.

**Timeout:** 300 seconds (5 minutes).

---

### `config/get` (Tool Call)

Retrieve configuration values.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "tools/call",
  "params": {
    "name": "config/get",
    "arguments": {
      "keys": ["model", "maxSteps"]
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "result": {
    "model": "gpt-4",
    "maxSteps": 10
  }
}
```

If `keys` is omitted or null, all configuration values are returned.

---

### `config/set` (Tool Call)

Update configuration values.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "tools/call",
  "params": {
    "name": "config/set",
    "arguments": {
      "values": {
        "model": "claude-3",
        "temperature": 0.8
      }
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": { "success": true }
}
```

The core may emit `config/changed` notifications for each changed key.

---

### `agent/run` (Tool Call)

Start an agent task.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "tools/call",
  "params": {
    "name": "agent/run",
    "arguments": {
      "agent": "code-analyzer",
      "goal": "Analyze the codebase for security vulnerabilities",
      "project": "/path/to/project"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "result": {
    "agent_id": "agent_1712345678_x1y2z3",
    "status": "started"
  }
}
```

Progress updates are delivered via `agent/progress` notifications. Completion via `agent/complete`. Errors via `agent/error`.

---

### `agent/status` (Tool Call)

Check the status of a running agent.

**Direction:** Extension → Core

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "tools/call",
  "params": {
    "name": "agent/status",
    "arguments": {
      "agent_id": "agent_1712345678_x1y2z3"
    }
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "agent_id": "agent_1712345678_x1y2z3",
    "status": "running",
    "step": 3,
    "total_steps": 10,
    "message": "Analyzing dependencies..."
  }
}
```

---

## Notification Schema

All notifications are sent from the **Core → Extension** (server → client).

### `chat/chunk`

Streaming text chunk from a chat response.

```json
{
  "jsonrpc": "2.0",
  "method": "chat/chunk",
  "params": {
    "session_id": "sess_1712345678_a1b2c3d",
    "chunk": "The function uses a recursive ",
    "done": false
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Identifies the chat session |
| `chunk` | string | Partial text content |
| `done` | boolean | `true` if this is the last chunk |

---

### `agent/progress`

Progress update during an agent task.

```json
{
  "jsonrpc": "2.0",
  "method": "agent/progress",
  "params": {
    "session_id": "sess_1712345678_a1b2c3d",
    "step": 3,
    "total": 10,
    "message": "Analyzing dependencies...",
    "status": "running"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Identifies the session |
| `step` | integer | Current step number |
| `total` | integer | Total number of steps |
| `message` | string | Human-readable progress description |
| `status` | string | One of: `"running"`, `"completed"`, `"failed"` |

---

### `agent/complete`

Task completion notification.

```json
{
  "jsonrpc": "2.0",
  "method": "agent/complete",
  "params": {
    "session_id": "sess_1712345678_a1b2c3d",
    "result": "The analysis found 3 potential issues...",
    "artifacts": ["/path/to/report.md", "/path/to/graph.dot"],
    "duration_ms": 45230
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Identifies the session |
| `result` | string | Final result text |
| `artifacts` | string[] | Paths to generated artifacts |
| `duration_ms` | integer | Total execution time in milliseconds |

---

### `agent/error`

Error notification during a task.

```json
{
  "jsonrpc": "2.0",
  "method": "agent/error",
  "params": {
    "session_id": "sess_1712345678_a1b2c3d",
    "error": "Failed to parse file: unexpected token",
    "suggestion": "Check that the file contains valid Rust code"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `session_id` | string | Identifies the session |
| `error` | string | Error description |
| `suggestion` | string? | Optional recovery suggestion |

---

### `config/changed`

Configuration value change notification.

```json
{
  "jsonrpc": "2.0",
  "method": "config/changed",
  "params": {
    "key": "model",
    "old_value": "gpt-4",
    "new_value": "claude-3"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `key` | string | Configuration key that changed |
| `old_value` | any | Previous value |
| `new_value` | any | New value |

---

## Lifecycle

### Startup Sequence

```
Extension                    Rust Core
    │                           │
    │  1. Read spire.corePath   │
    │  2. Spawn child process───│
    │                           │── 3. Initialize tracing
    │                           │── 4. Load embedding model
    │                           │── 5. Open knowledge graph
    │                           │── 6. Connect external MCP clients
    │                           │── 7. Start StdioTransport
    │  8. Read stdout line──────│
    │  9. Parse JSON-RPC msg    │
    │ 10. Mark as connected     │
    │                           │
    │ 11. Register notification │
    │     handlers              │
    │ 12. Register commands     │
    │ 13. Show status bar       │
    │                           │
    │ 14. Extension activated   │
```

### Shutdown Sequence

```
Extension                    Rust Core
    │                           │
    │  1. deactivate() called   │
    │  2. client.close()        │
    │  3. stdin.end()───────────│
    │  4. process.kill()────────│
    │                           │── 5. Process exits
    │  6. 'exit' event fires    │
```

### Config Change (Runtime Restart)

```
Extension                    Rust Core
    │                           │
    │  1. User changes          │
    │     spire.corePath        │
    │  2. client.close()        │
    │  3. Kill old process──────│
    │  4. client.start(newPath) │
    │  5. Spawn new process─────│
    │                           │── 6. New core initializes
    │  7. Connected             │
```

---

## Error Handling

### Timeouts

| Tool | Timeout | Rationale |
|------|---------|-----------|
| All tools except `chat/stream` | 60 seconds | Standard operations |
| `chat/stream` | 300 seconds (5 min) | Streaming responses may be long-running |

When a timeout fires, the pending promise is rejected with `"MCP request timed out"` and the request is removed from the pending map.

### Error Response Format

Tool call errors are returned as JSON-RPC errors:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "error": {
    "code": -32603,
    "message": "Tool call failed: Tool not found: 'unknown_tool'",
    "data": null
  }
}
```

### Connection Loss

If the Rust process exits unexpectedly:

1. All pending requests are rejected with `"MCP connection lost"`
2. The transport attempts reconnection with exponential backoff (up to 3 attempts)
3. If reconnection succeeds, the extension resumes normal operation
4. If all attempts fail, the extension continues running but all tool calls will throw `"MCP client not connected"`

### Stderr Logging

Any output on stderr from the Rust process is captured and displayed as a VS Code error notification:

```typescript
this.process.stderr?.on('data', (data) => {
  vscode.window.showErrorMessage(`Spire core: ${data.toString()}`);
});
```

---

## Sequence Diagrams

### Chat Streaming Session

```
ChatWebView    ChatService    McpClient       Rust Core
    │              │              │               │
    │  sendMessage │              │               │
    │─────────────>│              │               │
    │              │ callTool     │               │
    │              │ ("chat/stream")              │
    │              │─────────────>│               │
    │              │              │ tools/call    │
    │              │              │──────────────>│
    │              │              │               │── Process message
    │              │              │               │
    │              │              │ chat/chunk    │
    │              │              │<──────────────│
    │              │ onChunk      │               │
    │              │<─────────────│               │
    │  chunk       │              │               │
    │<─────────────│              │               │
    │              │              │ chat/chunk    │
    │              │              │<──────────────│
    │              │ onChunk      │               │
    │              │<─────────────│               │
    │  chunk       │              │               │
    │<─────────────│              │               │
    │              │              │ ...           │
    │              │              │               │
    │              │              │ agent/progress│
    │              │              │<──────────────│
    │              │ onProgress   │               │
    │              │<─────────────│               │
    │  progress    │              │               │
    │<─────────────│              │               │
    │              │              │               │
    │              │              │ agent/complete│
    │              │              │<──────────────│
    │              │ onComplete   │               │
    │              │<─────────────│               │
    │  complete    │              │               │
    │<─────────────│              │               │
```

### Agent Run

```
ConfigWebView   ConfigService   McpClient       Rust Core
    │              │              │               │
    │  runAgent    │              │               │
    │─────────────>│              │               │
    │              │ callTool     │               │
    │              │ ("agent/run")                │
    │              │─────────────>│               │
    │              │              │ tools/call    │
    │              │              │──────────────>│
    │              │              │               │── Start agent
    │              │              │  result       │
    │              │              │<──────────────│
    │              │<─────────────│               │
    │  agent_id    │              │               │
    │<─────────────│              │               │
    │              │              │               │
    │              │              │ agent/progress│
    │              │              │<──────────────│
    │              │ onProgress   │               │
    │              │<─────────────│               │
    │  progress    │              │               │
    │<─────────────│              │               │
    │              │              │ ...           │
    │              │              │               │
    │              │              │ agent/complete│
    │              │              │<──────────────│
    │              │ onComplete   │               │
    │              │<─────────────│               │
    │  complete    │              │               │
    │<─────────────│              │               │
```

### Config Change

```
ConfigWebView   ConfigService   McpClient       Rust Core
    │              │              │               │
    │  setConfig   │              │               │
    │─────────────>│              │               │
    │              │ callTool     │               │
    │              │ ("config/set")               │
    │              │─────────────>│               │
    │              │              │ tools/call    │
    │              │              │──────────────>│
    │              │              │               │── Update config
    │              │              │  result       │
    │              │              │<──────────────│
    │              │<─────────────│               │
    │              │              │               │
    │              │              │ config/changed│
    │              │              │<──────────────│
    │              │ onConfigChanged              │
    │              │<─────────────│               │
    │  config      │              │               │
    │  changed     │              │               │
    │<─────────────│              │               │
```

---

## Architecture Diagram

```
spire-rust/
│
├── ts/spire-extension/        ← TypeScript (UI + MCP Client)
│   ├── src/
│   │   ├── extension.ts       ← activate/deactivate, commands, status bar
│   │   ├── client/            ← MCP client & environment client
│   │   ├── server/            ← JSON-RPC server (router + handlers)
│   │   │   ├── transport.ts   ← stdio transport management
│   │   │   ├── router.ts      ← Request routing
│   │   │   └── handlers/      ← Tool handlers (workspace, git, editor, etc.)
│   │   ├── model/             ← Type definitions & message schemas
│   │   ├── util/              ← Utilities (logger)
│   │   └── webview/           ← Chat & config WebView UI
│   └── test/                  ← Integration tests
│
│   ════════════════════════════════════════  ← Interface boundary (stdio JSON-RPC 2.0)
│
├── rust/spire-core/           ← Rust (Actor System + MCP Client)
│   ├── src/
│   │   ├── main.rs            ← Entry point (StdioTransport + Actor System)
│   │   ├── lib.rs             ← Crate root
│   │   ├── framework/         ← Actor framework (actor, system, messages)
│   │   ├── actors/            ← Actor implementations
│   │   │   ├── coordinator.rs ← Workflow orchestrator
│   │   │   ├── chat.rs        ← Chat session management
│   │   │   ├── llm.rs         ← LLM gateway client
│   │   │   ├── tools.rs       ← Tool registry & execution
│   │   │   ├── vscode_tools.rs← VS Code tool bridge
│   │   │   ├── mcp_client.rs  ← External MCP server client
│   │   │   ├── progress.rs    ← Progress broadcaster
│   │   │   └── system.rs      ← System management
│   │   ├── mcp/               ← MCP protocol layer
│   │   │   └── client.rs      ← MCP client connection manager
│   │   └── transport/         ← stdio transport (JSON-RPC 2.0)
│   │       └── stdio.rs       ← Line-delimited JSON over stdin/stdout
│   └── tests/                 ← Integration tests
│
└── doc/                       ← Documentation
    ├── extension-core-interface.md  ← THIS DOCUMENT
    ├── messages-and-types.md        ← Actor message reference
    ├── graph-schema.md              ← Knowledge graph schema
    └── agent-infrastructure.md      ← Agent system design
```

---

## Related

- [Root README](../README.md) — Project overview and quick start
- [spire-core README](../rust/spire-core/README.md) — Rust core documentation
- [spire-extension README](../ts/spire-extension/README.md) — Extension documentation
- [messages-and-types.md](messages-and-types.md) — Actor message reference
