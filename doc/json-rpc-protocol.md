# JSON-RPC Protocol for VS Code Environment Server

> Version 1.0 — July 2026

## 1. Transport

The VS Code Environment Server communicates over **stdin/stdout** using
**newline-delimited JSON-RPC 2.0**.

| Direction | Stream | Format |
|-----------|--------|--------|
| Client → Server | stdin | `{"jsonrpc":"2.0","id":1,"method":"xxx","params":{...}}\n` |
| Server → Client | stdout | `{"jsonrpc":"2.0","id":1,"result":{...}}\n` |
| Server → Client (error) | stdout | `{"jsonrpc":"2.0","id":1,"error":{"code":-32603,"message":"..."}}\n` |
| Server → Client (event) | stdout | `{"jsonrpc":"2.0","method":"event/xxx","params":{...}}\n` |
| Server logs | stderr | Human-readable log lines (never JSON-RPC) |

### 1.1 Request

```json
{"jsonrpc":"2.0","id":1,"method":"chat/getActive","params":{}}
```

- `id` — number, used to correlate responses
- `method` — string, one of the methods listed below
- `params` — object or omitted

### 1.2 Response

```json
{"jsonrpc":"2.0","id":1,"result":{"id":"default","title":"New Chat",...}}
```

### 1.3 Error Response

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}
```

### 1.4 Notification (Event)

```json
{"jsonrpc":"2.0","method":"event/editor/activeEditorChanged","params":{"editor":null}}
```

Notifications have no `id` — the server does not expect a response.

---

## 2. Error Codes

| Code | Meaning |
|------|---------|
| `-32700` | Parse error — invalid JSON |
| `-32600` | Invalid request — missing `jsonrpc` or `method` |
| `-32601` | Method not found |
| `-32602` | Invalid params |
| `-32603` | Internal error |
| `-32000` | VS Code API error |
| `-32001` | Resource not found (e.g., document not open) |

---

## 3. Methods

### 3.1 Chat

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `chat/getActive` | `void` | `ChatDialog \| null` | Get the currently active chat dialog |
| `chat/getHistory` | `void` | `ChatDialog[]` | List all chat dialogs |
| `chat/getMessage` | `{ chatId, messageId }` | `ChatMessage \| null` | Get a specific message |
| `chat/append` | `{ chatId, content, options? }` | `ChatMessage` | Append a message to a chat |
| `chat/clear` | `{ chatId }` | `void` | Clear all messages in a chat |
| `chat/setTitle` | `{ chatId, title }` | `void` | Set the chat title |
| `chat/show` | `{ chatId?, panel? }` | `void` | Focus the chat panel |

### 3.2 Workspace

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `workspace/getFolders` | `void` | `WorkspaceFolder[]` | List workspace folders |
| `workspace/searchFiles` | `{ pattern, options? }` | `string[]` | Search for files by glob pattern |
| `workspace/searchText` | `{ pattern, options? }` | `SearchMatch[]` | Search file contents by text pattern |

### 3.3 Editor

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `editor/getActive` | `void` | `TextEditor \| null` | Get the active text editor |
| `editor/getVisible` | `void` | `TextEditor[]` | Get all visible editors |
| `editor/openFile` | `{ uri, options? }` | `void` | Open a file in an editor |
| `editor/close` | `{ uri }` | `void` | Close an editor tab |
| `editor/setSelection` | `{ uri, selection }` | `void` | Set the selection in an editor |
| `editor/revealRange` | `{ uri, range }` | `void` | Reveal a range in the editor |

### 3.4 Document

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `document/read` | `{ uri, options? }` | `TextDocument` | Read a document's content |
| `document/insertText` | `{ uri, position, text }` | `void` | Insert text at a position |
| `document/replaceText` | `{ uri, range, text }` | `void` | Replace text in a range |
| `document/deleteRange` | `{ uri, range }` | `void` | Delete text in a range |
| `document/format` | `{ uri }` | `void` | Format a document |
| `document/applyEdit` | `{ uri, edits }` | `boolean` | Apply multiple text edits |

### 3.5 Diagnostics

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `diagnostics/get` | `{ uri?, severity? }` | `Diagnostic[]` | Get diagnostics (problems) |

### 3.6 Terminal

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `terminal/list` | `void` | `Terminal[]` | List all terminals |
| `terminal/create` | `{ name, options? }` | `string` | Create a new terminal (returns ID) |
| `terminal/send` | `{ terminalId, text, options? }` | `void` | Send text to a terminal |
| `terminal/dispose` | `{ terminalId }` | `void` | Dispose a terminal |

### 3.7 Git

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `git/getChanges` | `{ staged?, uri? }` | `GitChange[]` | Get git working tree changes |

### 3.8 Symbols (Code Intelligence)

| Method | Params | Result | Description |
|--------|--------|--------|-------------|
| `symbols/goToDefinition` | `{ uri, position }` | `Symbol \| null` | Go to definition |
| `symbols/findReferences` | `{ uri, position }` | `Symbol[]` | Find references |
| `symbols/getHover` | `{ uri, position }` | `HoverInfo \| null` | Get hover information |

---

## 4. Events (Server → Client Notifications)

| Event | When | Payload |
|-------|------|---------|
| `event/editor/activeEditorChanged` | User switches editor tab | `{ editor: TextEditor \| null }` |
| `event/editor/textChanged` | Document content changes | `{ uri, range, text }` |
| `event/editor/selectionChanged` | Selection changes | `{ uri, selections }` |
| `event/diagnostics/changed` | Diagnostics update | `{ diagnostics }` |
| `event/workspace/foldersChanged` | Workspace folders change | `{ folders }` |
| `event/git/changed` | Git state changes | `{ changes }` |

---

## 5. Domain Types

See [`vscode-environment-model.md`](./vscode-environment-model.md) for the full
domain model definitions. The TypeScript interfaces in
`spire-extension/src/model/types.ts` are the canonical type definitions.

---

## 6. Example Session

```
→ {"jsonrpc":"2.0","id":1,"method":"workspace/getFolders","params":{}}
← {"jsonrpc":"2.0","id":1,"result":[{"name":"spire-rust","uri":"file:///Users/steve/naturesense/tools/spire-rust","isActive":true}]}

→ {"jsonrpc":"2.0","id":2,"method":"editor/getActive","params":{}}
← {"jsonrpc":"2.0","id":2,"result":{"document":{"uri":"file:///...","fileName":"main.rs","languageId":"rust","lineCount":120,"isDirty":false,"isUntitled":false},"viewColumn":1,"selections":[],"visibleRanges":[]}}

→ {"jsonrpc":"2.0","id":3,"method":"chat/append","params":{"chatId":"default","content":"Hello!","options":{"role":"user"}}}
← {"jsonrpc":"2.0","id":3,"result":{"id":"msg-abc123","role":"user","content":"Hello!","timestamp":"2026-07-09T..."}}

← {"jsonrpc":"2.0","method":"event/editor/activeEditorChanged","params":{"editor":null}}
```
