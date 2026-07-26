// ──────────────────────────────────────────────
// 1. CHAT DIALOG
// ──────────────────────────────────────────────

export interface ChatDialog {
  id: string;
  title: string;
  messages: ChatMessage[];
  status: 'idle' | 'streaming' | 'waiting_for_tool';
  model?: string;
  createdAt: string; // ISO timestamp
  updatedAt: string; // ISO timestamp
}

export interface ChatMessage {
  id: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  timestamp: string;
  toolCalls?: ToolCallInvocation[];
  toolResults?: ToolResult[];
  isStreaming?: boolean;
  metadata?: Record<string, unknown>;
  widget?: ChatWidget;  // NEW: embedded interactive widget
}

// ──────────────────────────────────────────────
// 1b. CHAT WIDGETS (interactive display components)
// ──────────────────────────────────────────────

export type WidgetType = 'chat-message' | 'build-list' | 'radio-group' | 'checkbox-list' | 'progress-bar';

export interface ChatWidget {
  widgetId: string;
  widgetType: WidgetType;
  state: Record<string, unknown>;
}

// ── Build list widget ─────────────────────────

export interface BuildListState {
  title?: string;
  builds: BuildItem[];
}

export interface BuildItem {
  name: string;
  status: 'pending' | 'running' | 'success' | 'error' | 'skipped';
  duration_ms?: number;
  log?: string;
}

// ── Radio group widget ────────────────────────

export interface RadioGroupState {
  label?: string;
  options: RadioOption[];
  selected: string | null;
}

export interface RadioOption {
  value: string;
  label: string;
  disabled?: boolean;
}

// ── Checkbox list widget ──────────────────────

export interface CheckboxListState {
  label?: string;
  options: CheckboxOption[];
  selected: string[];
}

export interface CheckboxOption {
  value: string;
  label: string;
}

// ── Chat message widget (default for plain text messages) ──

export interface ChatMessageState {
  role: 'user' | 'assistant' | 'system' | 'error';
  content: string;
  intent?: { name: string; route: string; confidence: number };
  timestamp?: string;
}

// ── Progress bar widget ───────────────────────

export interface ProgressBarState {
  label?: string;
  value: number;   // 0–100
  status?: 'running' | 'success' | 'error';
}

export interface ToolCallInvocation {
  toolName: string;
  args: Record<string, unknown>;
  status: 'pending' | 'running' | 'completed' | 'failed';
  result?: unknown;
  startTime?: string;
  endTime?: string;
}

export interface ToolResult {
  toolName: string;
  success: boolean;
  output: unknown;
  durationMs?: number;
}

// ──────────────────────────────────────────────
// 2. WORKSPACE
// ──────────────────────────────────────────────

export interface WorkspaceFolder {
  name: string;
  uri: string;
  isActive: boolean;
}

// ──────────────────────────────────────────────
// 3. TEXT DOCUMENT
// ──────────────────────────────────────────────

export interface TextDocument {
  uri: string;
  fileName: string;
  languageId: string;
  lineCount: number;
  isDirty: boolean;
  isUntitled: boolean;
  text?: string;
}

// ──────────────────────────────────────────────
// 4. TEXT EDITOR
// ──────────────────────────────────────────────

export interface TextEditor {
  document: TextDocument;
  viewColumn: number;
  selections: Selection[];
  visibleRanges: Range[];
}

export interface Selection {
  start: Position;
  end: Position;
  isEmpty: boolean;
  text?: string;
}

export interface Position {
  line: number;      // 0-based
  character: number; // 0-based column
}

export interface Range {
  start: Position;
  end: Position;
}

// ──────────────────────────────────────────────
// 5. DIAGNOSTIC
// ──────────────────────────────────────────────

export interface Diagnostic {
  uri: string;
  range: Range;
  severity: 'error' | 'warning' | 'information' | 'hint';
  message: string;
  source?: string;
  code?: string;
}

// ──────────────────────────────────────────────
// 6. TERMINAL
// ──────────────────────────────────────────────

export interface Terminal {
  id: string;
  name: string;
  shellPath: string;
  isVisible: boolean;
  exitCode?: number;
  lastOutput?: string;
}

// ──────────────────────────────────────────────
// 7. GIT CHANGE
// ──────────────────────────────────────────────

export interface GitChange {
  uri: string;
  originalFileName?: string;
  status: 'added' | 'modified' | 'deleted' | 'renamed';
  staged: boolean;
  diff?: string;
}

// ──────────────────────────────────────────────
// 8. SYMBOL
// ──────────────────────────────────────────────

export interface Symbol {
  name: string;
  kind: 'function' | 'class' | 'variable' | 'method' | 'interface'
      | 'enum' | 'module' | 'property' | 'constant';
  uri: string;
  range: Range;
  selectionRange: Range;
  containerName?: string;
}

// ──────────────────────────────────────────────
// 9. OPTION / FILTER TYPES
// ──────────────────────────────────────────────

export interface SearchOptions {
  include?: string;
  exclude?: string;
}

export interface TextSearchOptions {
  include?: string;
  maxResults?: number;
  contextLines?: number;
}

export interface RangeOptions {
  startLine?: number;
  endLine?: number;
}

export interface DiagnosticFilter {
  uri?: string;
  severity?: Diagnostic['severity'];
}

export interface GitFilter {
  staged?: boolean;
  uri?: string;
}

export interface SearchMatch {
  uri: string;
  line: number;
  column: number;
  lineContent: string;
  context?: string[];
}

export interface HoverInfo {
  contents: string;
  range?: Range;
}

export interface AppendOptions {
  role?: ChatMessage['role'];
  metadata?: Record<string, unknown>;
}

export interface ShowChatOptions {
  chatId?: string;
  panel?: boolean;
}

export interface OpenFileOptions {
  line?: number;
  column?: number;
  viewColumn?: number;
  preview?: boolean;
}

export interface TerminalOptions {
  cwd?: string;
  env?: Record<string, string>;
}

export interface SendOptions {
  addNewline?: boolean;
}

export interface TextEdit {
  range: Range;
  newText: string;
}
