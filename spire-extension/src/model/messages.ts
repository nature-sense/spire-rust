import type {
  ChatDialog,
  ChatMessage,
  WorkspaceFolder,
  TextEditor,
  TextDocument,
  Diagnostic,
  Terminal,
  GitChange,
  Symbol,
  Position,
  Range,
  SearchOptions,
  TextSearchOptions,
  RangeOptions,
  DiagnosticFilter,
  GitFilter,
  SearchMatch,
  HoverInfo,
  AppendOptions,
  ShowChatOptions,
  OpenFileOptions,
  TerminalOptions,
  SendOptions,
  TextEdit,
} from './types';

// ──────────────────────────────────────────────
// JSON-RPC 2.0 ENVELOPES
// ──────────────────────────────────────────────

export interface JsonRpcRequest {
  jsonrpc: '2.0';
  id: number;
  method: string;
  params?: unknown;
}

export interface JsonRpcResponse {
  jsonrpc: '2.0';
  id: number;
  result?: unknown;
  error?: JsonRpcError;
}

export interface JsonRpcNotification {
  jsonrpc: '2.0';
  method: string;
  params?: unknown;
}

export interface JsonRpcError {
  code: number;
  message: string;
  data?: unknown;
}

// Standard JSON-RPC error codes
export const ErrorCode = {
  ParseError: -32700,
  InvalidRequest: -32600,
  MethodNotFound: -32601,
  InvalidParams: -32602,
  InternalError: -32603,
  VscodeApiError: -32000,
  ResourceNotFound: -32001,
} as const;

// ──────────────────────────────────────────────
// METHOD → PARAMS / RESULT TYPE MAP
// ──────────────────────────────────────────────

export interface MethodMap {
  // ── Chat ──
  'chat/getActive': { params: void; result: ChatDialog | null };
  'chat/getHistory': { params: void; result: ChatDialog[] };
  'chat/getMessage': { params: { chatId: string; messageId: string }; result: ChatMessage | null };
  'chat/append': { params: { chatId: string; content: string; options?: AppendOptions }; result: ChatMessage };
  'chat/clear': { params: { chatId: string }; result: void };
  'chat/setTitle': { params: { chatId: string; title: string }; result: void };
  'chat/show': { params: ShowChatOptions; result: void };

  // ── Workspace ──
  'workspace/getFolders': { params: void; result: WorkspaceFolder[] };
  'workspace/searchFiles': { params: { pattern: string; options?: SearchOptions }; result: string[] };
  'workspace/searchText': { params: { pattern: string; options?: TextSearchOptions }; result: SearchMatch[] };

  // ── Editor ──
  'editor/getActive': { params: void; result: TextEditor | null };
  'editor/getVisible': { params: void; result: TextEditor[] };
  'editor/openFile': { params: { uri: string; options?: OpenFileOptions }; result: void };
  'editor/close': { params: { uri: string }; result: void };
  'editor/setSelection': { params: { uri: string; selection: { start: Position; end: Position } }; result: void };
  'editor/revealRange': { params: { uri: string; range: Range }; result: void };

  // ── Document ──
  'document/read': { params: { uri: string; options?: RangeOptions }; result: TextDocument };
  'document/insertText': { params: { uri: string; position: Position; text: string }; result: void };
  'document/replaceText': { params: { uri: string; range: Range; text: string }; result: void };
  'document/deleteRange': { params: { uri: string; range: Range }; result: void };
  'document/format': { params: { uri: string }; result: void };
  'document/applyEdit': { params: { uri: string; edits: TextEdit[] }; result: boolean };

  // ── Diagnostics ──
  'diagnostics/get': { params: DiagnosticFilter; result: Diagnostic[] };

  // ── Terminal ──
  'terminal/list': { params: void; result: Terminal[] };
  'terminal/create': { params: { name: string; options?: TerminalOptions }; result: string };
  'terminal/send': { params: { terminalId: string; text: string; options?: SendOptions }; result: void };
  'terminal/dispose': { params: { terminalId: string }; result: void };

  // ── Git ──
  'git/getChanges': { params: GitFilter; result: GitChange[] };

  // ── Symbols ──
  'symbols/goToDefinition': { params: { uri: string; position: Position }; result: Symbol | null };
  'symbols/findReferences': { params: { uri: string; position: Position }; result: Symbol[] };
  'symbols/getHover': { params: { uri: string; position: Position }; result: HoverInfo | null };
}

// ──────────────────────────────────────────────
// EVENT NOTIFICATION TYPES
// ──────────────────────────────────────────────

export interface EventMap {
  'event/editor/activeEditorChanged': { editor: TextEditor | null };
  'event/editor/textChanged': { uri: string; range: Range; text: string };
  'event/editor/selectionChanged': { uri: string; selections: import('./types').Selection[] };
  'event/diagnostics/changed': { diagnostics: Diagnostic[] };
  'event/workspace/foldersChanged': { folders: WorkspaceFolder[] };
  'event/git/changed': { changes: GitChange[] };
}

// ──────────────────────────────────────────────
// HELPER: Extract param/result types
// ──────────────────────────────────────────────

export type MethodName = keyof MethodMap;
export type EventName = keyof EventMap;

export type MethodParams<M extends MethodName> = MethodMap[M]['params'];
export type MethodResult<M extends MethodName> = MethodMap[M]['result'];
export type EventParams<E extends EventName> = EventMap[E];
