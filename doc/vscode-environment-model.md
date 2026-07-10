# VS Code Environment Domain Model

> Abstract model for representing and manipulating the VS Code working environment
> through the VscodeTool bridge.

---

## 1. Domain Objects

### 1.1 Chat Dialog

The AI conversation panel itself — the medium through which the agent communicates
and a manipulable domain object.

```typescript
interface ChatDialog {
  id: string;                        // unique session id
  title: string;                     // auto-generated or user-set title
  messages: ChatMessage[];
  status: 'idle' | 'streaming' | 'waiting_for_tool';
  model?: string;                    // which LLM is being used
  createdAt: string;                 // ISO timestamp
  updatedAt: string;
}

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;                   // markdown text
  timestamp: string;
  toolCalls?: ToolCallInvocation[];
  toolResults?: ToolResult[];
  isStreaming?: boolean;
  metadata?: Record<string, unknown>;
}

interface ToolCallInvocation {
  toolName: string;
  args: Record<string, unknown>;
  status: 'pending' | 'running' | 'completed' | 'failed';
  result?: unknown;
  startTime?: string;
  endTime?: string;
}

interface ToolResult {
  toolName: string;
  success: boolean;
  output: unknown;
  durationMs?: number;
}
```

### 1.2 Workspace

```typescript
interface WorkspaceFolder {
  name: string;          // "spire-rust"
  uri: string;           // "file:///Users/steve/naturesense/tools/spire-rust"
  isActive: boolean;
}
```

### 1.3 Text Document

```typescript
interface TextDocument {
  uri: string;
  fileName: string;
  languageId: string;                // "rust", "typescript", "markdown"…
  lineCount: number;
  isDirty: boolean;
  isUntitled: boolean;
  text?: string;                     // full content (omitted for very large files)
}
```

### 1.4 Text Editor

```typescript
interface TextEditor {
  document: TextDocument;
  viewColumn: number;
  selections: Selection[];
  visibleRanges: Range[];
}

interface Selection {
  start: Position;
  end: Position;
  isEmpty: boolean;
  text?: string;
}

interface Position {
  line: number;                      // 0-based
  character: number;                 // 0-based column
}

interface Range {
  start: Position;
  end: Position;
}
```

### 1.5 Diagnostic (Problem Marker)

```typescript
interface Diagnostic {
  uri: string;
  range: Range;
  severity: 'error' | 'warning' | 'information' | 'hint';
  message: string;
  source?: string;                   // e.g. "rustc", "typescript"
  code?: string;                     // e.g. "E0308"
}
```

### 1.6 Terminal (Shell Session)

```typescript
interface Terminal {
  id: string;
  name: string;
  shellPath: string;
  isVisible: boolean;
  exitCode?: number;
  lastOutput?: string;
}
```

### 1.7 Git Change

```typescript
interface GitChange {
  uri: string;
  originalFileName?: string;
  status: 'added' | 'modified' | 'deleted' | 'renamed';
  staged: boolean;
  diff?: string;                     // unified diff text
}
```

### 1.8 Symbol (Code Intelligence)

```typescript
interface Symbol {
  name: string;
  kind: 'function' | 'class' | 'variable' | 'method' | 'interface'
      | 'enum' | 'module' | 'property' | 'constant';
  uri: string;
  range: Range;
  selectionRange: Range;
  containerName?: string;            // enclosing class/struct
}
```

---

## 2. Query Functions (Observation — Read State)

```typescript
interface VscodeEnvironmentReader {
  // ── Chat Dialog ──
  getActiveChat(): Promise<ChatDialog | null>;
  getChatHistory(): Promise<ChatDialog[]>;
  getChatMessage(chatId: string, messageId: string): Promise<ChatMessage | null>;

  // ── Workspace ──
  getWorkspaceFolders(): Promise<WorkspaceFolder[]>;
  searchFiles(pattern: string, options?: SearchOptions): Promise<string[]>;
  searchText(pattern: string, options?: TextSearchOptions): Promise<SearchMatch[]>;

  // ── Editors & Documents ──
  getActiveEditor(): Promise<TextEditor | null>;
  getVisibleEditors(): Promise<TextEditor[]>;
  readDocument(uri: string, options?: RangeOptions): Promise<TextDocument>;

  // ── Diagnostics ──
  getDiagnostics(options?: DiagnosticFilter): Promise<Diagnostic[]>;

  // ── Terminals ──
  listTerminals(): Promise<Terminal[]>;

  // ── Git ──
  getGitChanges(options?: GitFilter): Promise<GitChange[]>;

  // ── Symbols ──
  findDefinition(uri: string, position: Position): Promise<Symbol | null>;
  findReferences(uri: string, position: Position): Promise<Symbol[]>;
  getHoverInfo(uri: string, position: Position): Promise<HoverInfo | null>;
}

// Supporting types
interface SearchOptions       { include?: string; exclude?: string; }
interface TextSearchOptions  { include?: string; maxResults?: number; contextLines?: number; }
interface RangeOptions       { startLine?: number; endLine?: number; }
interface DiagnosticFilter   { uri?: string; severity?: Diagnostic['severity']; }
interface GitFilter          { staged?: boolean; uri?: string; }
interface SearchMatch        { uri: string; line: number; column: number; lineContent: string; context?: string[]; }
interface HoverInfo          { contents: string; range?: Range; }
```

---

## 3. Command Functions (Mutation — Change State)

```typescript
interface VscodeEnvironmentWriter {
  // ── Chat Dialog ──
  appendToChat(chatId: string, content: string, options?: AppendOptions): Promise<ChatMessage>;
  clearChat(chatId: string): Promise<void>;
  setChatTitle(chatId: string, title: string): Promise<void>;
  showChat(options?: ShowChatOptions): Promise<void>;

  // ── Editors ──
  openFile(uri: string, options?: OpenFileOptions): Promise<void>;
  closeEditor(uri: string): Promise<void>;
  setSelection(uri: string, selection: { start: Position; end: Position }): Promise<void>;
  revealRange(uri: string, range: Range): Promise<void>;

  // ── Document Editing ──
  insertText(uri: string, position: Position, text: string): Promise<void>;
  replaceText(uri: string, range: Range, text: string): Promise<void>;
  deleteRange(uri: string, range: Range): Promise<void>;
  formatDocument(uri: string): Promise<void>;
  applyEdit(uri: string, edits: TextEdit[]): Promise<boolean>;

  // ── Terminals ──
  createTerminal(name: string, options?: TerminalOptions): Promise<string>;  // returns terminal ID
  sendToTerminal(terminalId: string, text: string, options?: SendOptions): Promise<void>;
  disposeTerminal(terminalId: string): Promise<void>;
}

// Supporting types
interface AppendOptions   { role?: ChatMessage['role']; metadata?: Record<string, unknown>; }
interface ShowChatOptions { chatId?: string; panel?: boolean; }
interface OpenFileOptions { line?: number; column?: number; viewColumn?: number; preview?: boolean; }
interface TerminalOptions { cwd?: string; env?: Record<string, string>; }
interface SendOptions     { addNewline?: boolean; }
interface TextEdit        { range: Range; newText: string; }
```

---

## 4. Tool Mappings

Each tool exposed to the AI agent maps to one query or command function:

### Phase 1 — Core Coding Assistant

| Tool Name | Type | Calls |
|-----------|------|-------|
| `vscode_read_active_editor` | Query | `getActiveEditor()` |
| `vscode_get_selection` | Query | `getActiveEditor()` → selections |
| `vscode_open_file` | Command | `openFile(uri, opts)` |
| `vscode_get_workspace_folders` | Query | `getWorkspaceFolders()` |
| `vscode_search_files` | Query | `searchFiles(pattern)` |
| `vscode_get_diagnostics` | Query | `getDiagnostics({ uri, severity })` |
| `vscode_get_git_changes` | Query | `getGitChanges()` |

### Phase 2 — Richer Interaction

| Tool Name | Type | Calls |
|-----------|------|-------|
| `vscode_create_terminal` | Command | `createTerminal(name)` |
| `vscode_send_to_terminal` | Command | `sendToTerminal(id, text)` |
| `vscode_list_terminals` | Query | `listTerminals()` |
| `vscode_get_visible_editors` | Query | `getVisibleEditors()` |
| `vscode_search_text` | Query | `searchText(pattern, opts)` |

### Phase 3 — Code Intelligence

| Tool Name | Type | Calls |
|-----------|------|-------|
| `vscode_go_to_definition` | Query | `findDefinition(uri, pos)` |
| `vscode_find_references` | Query | `findReferences(uri, pos)` |
| `vscode_get_hover` | Query | `getHoverInfo(uri, pos)` |

### Phase 4 — Document Editing

| Tool Name | Type | Calls |
|-----------|------|-------|
| `vscode_insert_text` | Command | `insertText(uri, pos, text)` |
| `vscode_replace_text` | Command | `replaceText(uri, range, text)` |
| `vscode_format_document` | Command | `formatDocument(uri)` |

### Phase 5 — Chat Dialog

| Tool Name | Type | Calls |
|-----------|------|-------|
| `vscode_get_active_chat` | Query | `getActiveChat()` |
| `vscode_get_chat_history` | Query | `getChatHistory()` |
| `vscode_append_to_chat` | Command | `appendToChat(chatId, content)` |
| `vscode_clear_chat` | Command | `clearChat(chatId)` |
| `vscode_set_chat_title` | Command | `setChatTitle(chatId, title)` |
| `vscode_show_chat` | Command | `showChat({ chatId })` |

---

## 5. Architecture Notes

### Chat Dialog: Medium vs Object

The chat dialog plays a dual role:

| Role | Mechanism | Tool Support |
|------|-----------|-------------|
| **Medium** | Agent natively streams tokens to chat via `ChatService` actor | Not a tool — primary output channel |
| **Object** | Agent reads history, appends pre-formatted content, organises conversations | `vscode_get_active_chat`, `vscode_append_to_chat`, etc. |

### Reader/Writer Separation

- **Query functions** are safe to call repeatedly — they merely observe state.
- **Command functions** have side effects and should be used deliberately.
- This maps cleanly to how the AI agent reasons about the environment.

### Implementation Path

Each function becomes a closure wrapped in the existing `registerVscodeTool()` → `VscodeTool` → `ToolsManagerActor` path:

1. TS side calls `coreHandle.registerVscodeTool(name, desc, schema, callback)`.
2. The callback wraps the VS Code API call.
3. Rust side stores it as a `VscodeTool` under the `"vscode-extension"` server.
4. The agent invokes it like any other tool via `ToolsManagerActor`.
