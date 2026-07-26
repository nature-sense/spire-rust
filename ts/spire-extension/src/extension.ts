import * as vscode from 'vscode';
import * as path from 'path';
import { Router } from './server/router';
import { BidirectionalClient } from './client/bidirectional-client';
import { ChatSidebarProvider } from './webview/sidebar-provider';
import { chatHandlers, setChatNotifier } from './server/handlers/chat';
import { workspaceHandlers } from './server/handlers/workspace';
import { editorHandlers } from './server/handlers/editor';
import { diagnosticsHandlers } from './server/handlers/diagnostics';
import { terminalHandlers } from './server/handlers/terminal';
import { gitHandlers } from './server/handlers/git';
import { symbolHandlers } from './server/handlers/symbols';
import { documentHandlers } from './server/handlers/document';
import { FileWatcher } from './server/handlers/file-watcher';
import { logger } from './util/logger';

/**
 * Spire Extension — Main Entry Point
 *
 * Architecture:
 *   Extension Host (this process)          Subprocess (Rust core)
 *   ┌─────────────────────────────┐       ┌──────────────────────┐
 *   │ BidirectionalClient         │ stdin │ Transport + Router   │
 *   │   ├── outgoing reqs ────────┼──────►│   ├── agents         │
 *   │   ├── incoming reqs ◄───────┼───────┤   ├── chat           │
 *   │   └── local Router         │ stdout │   └── tools          │
 *   │       ├── workspace.ts     │       └──────────────────────┘
 *   │       ├── editor.ts        │
 *   │       ├── diagnostics.ts   │
 *   │       ├── terminal.ts      │
 *   │       ├── git.ts           │
 *   │       ├── symbols.ts       │
 *   │       ├── document.ts      │
 *   │       └── chat.ts (local)  │
 *   ├── Sidebar Webview          │
 *   └── Chat service abstraction │
 *   └─────────────────────────────┘
 *
 * The extension is a thin shim providing VS Code-specific services to the
 * subprocess which is the core of the application. The subprocess (Rust)
 * handles agents, chat logic, tools, etc. and calls back to the extension
 * for VS Code API operations via the bidirectional JSON-RPC channel.
 */

let client: BidirectionalClient | null = null;
let localRouter: Router | null = null;
let sidebarProvider: ChatSidebarProvider | null = null;
let webviewPanel: vscode.WebviewPanel | null = null;
let fileWatcher: FileWatcher | null = null;
let disposables: vscode.Disposable[] = [];

let webviewReady = false;

/**
 * Activate the extension.
 */
export function activate(context: vscode.ExtensionContext): void {
  logger.info('Spire Extension activating...');

  // Initialize the local router with VS Code API handlers
  initializeLocalRouter();

  // Start the subprocess (Rust core)
  startSubprocess(context);


  // Register the sidebar provider
  sidebarProvider = new ChatSidebarProvider(context.extensionUri);
  sidebarProvider.setMessageHandler((message) => handleWebviewMessage(message as any));

  context.subscriptions.push(
    vscode.window.registerWebviewViewProvider(ChatSidebarProvider.viewType, sidebarProvider)
  );

  // Register the "Spire: Open Chat" command (opens a tab panel as fallback)
  const openChatCommand = vscode.commands.registerCommand('spire.openChat', () => {
    openChatWebview(context);
  });

  // Register the "Spire: Restart Environment Server" command
  const restartCommand = vscode.commands.registerCommand('spire.restartEnvServer', async () => {
    logger.info('Restarting subprocess...');
    await restartSubprocess();
    postMessageToAllWebviews({ type: 'status', connected: client?.isRunning ?? false });
  });

  context.subscriptions.push(openChatCommand, restartCommand);

  logger.info('Spire Extension activated successfully');
}

/**
 * Deactivate the extension.
 */
export async function deactivate(): Promise<void> {
  logger.info('Spire Extension deactivating...');

  // Stop the file watcher
  if (fileWatcher) {
    fileWatcher.stop();
    fileWatcher = null;
  }

  // Stop the subprocess
  if (client) {
    await client.stop();
    client = null;
  }

  // Clean up
  disposables.forEach(d => d.dispose());
  disposables = [];

  logger.info('Spire Extension deactivated');
}

// ── Local Router Initialization ─────────────────────────────────────────────

function initializeLocalRouter(): void {
  localRouter = new Router();

  // Register all VS Code API handler modules
  localRouter.registerAll(chatHandlers);
  localRouter.registerAll(workspaceHandlers);
  localRouter.registerAll(editorHandlers);
  localRouter.registerAll(diagnosticsHandlers);
  localRouter.registerAll(terminalHandlers);
  localRouter.registerAll(gitHandlers);
  localRouter.registerAll(symbolHandlers);
  localRouter.registerAll(documentHandlers);

  // Wire up chat event notifications to webview
  setChatNotifier((method, params) => {
    postMessageToAllWebviews({
      type: 'notification',
      method,
      params,
    });
  });

  logger.info('Local router initialized with VS Code API handlers');
}

// ── Subprocess Management ───────────────────────────────────────────────────

function startSubprocess(context: vscode.ExtensionContext): void {
  // Use the Rust binary for both development and production
  const command = path.join(context.extensionPath, 'bin', 'darwin-arm64', 'spire-core');
  const args: string[] = [];

  // Set SPIRE_PROJECT_ROOT to the workspace root so the core doesn't scan /
  const env: Record<string, string> = { ...process.env } as Record<string, string>;
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (workspaceFolders && workspaceFolders.length > 0) {
    env['SPIRE_PROJECT_ROOT'] = workspaceFolders[0].uri.fsPath;
  }

  client = new BidirectionalClient({
    command,
    args,
    env,
    autoRestart: true,
    // 120s timeout to match the Rust LlmActor's reqwest timeout (Duration::from_secs(120)).
    // LLM completions can take >60s, especially with many tool definitions or slow API responses.
    timeout: 120_000,
  });


  // Register the local router so the client can handle incoming requests
  // from the subprocess (VS Code API calls)
  if (localRouter) {
    client.setLocalRouter(localRouter);
  }

  // Handle notifications from the subprocess
  client.onAnyNotification((method, params) => {
    logger.debug(`Notification from subprocess: ${method}`);

    // Forward relevant notifications to the webview
    if (method.startsWith('event/')) {
      const message = {
        type: 'notification' as const,
        method,
        params,
      };

      postMessageToAllWebviews(message);
    }
  });

  // Start the subprocess
  client.start().then(() => {
    logger.info('Subprocess connected — notifying webviews');
    postMessageToAllWebviews({ type: 'status', connected: true });

    // Start the file watcher for continuous project sync (Phase 3)
    startFileWatcher();
  }).catch((err) => {
    logger.error(`Failed to start subprocess: ${err.message}`);
    postMessageToAllWebviews({ type: 'status', connected: false, error: err.message });
  });

}

/**
 * Start the file system watcher for continuous project sync.
 *
 * This is Phase 3 of the three-phase project sync:
 *   1. Bootstrap (cold start) — full scan + create-all
 *   2. Startup sync (warm start) — content-hash manifest diff
 *   3. Continuous sync (real-time) — file change events from this watcher
 *
 * The watcher forwards file system events to the Rust core as
 * `project/fileEvent` notifications, which the ProjectSync actor
 * processes to update the graph database in real-time.
 */
function startFileWatcher(): void {
  const workspaceFolders = vscode.workspace.workspaceFolders;
  if (!workspaceFolders || workspaceFolders.length === 0) {
    logger.warn('FileWatcher: no workspace folder, skipping');
    return;
  }

  const workspacePath = workspaceFolders[0].uri.fsPath;

  // Create the notify callback that sends events to the core subprocess
  const notifyCore = (method: string, params: unknown) => {
    if (client && client.isRunning) {
      client.notify(method, params);
    }
  };

  fileWatcher = new FileWatcher(workspacePath, notifyCore);
  fileWatcher.start();

  logger.info('FileWatcher: started for continuous project sync');
}

async function restartSubprocess(): Promise<void> {
  if (client) {
    await client.restart();
    postMessageToAllWebviews({ type: 'status', connected: client.isRunning });
  }
}

// ── Webview (Tab Panel Fallback) ────────────────────────────────────────────

function openChatWebview(context: vscode.ExtensionContext): void {
  // If panel already exists, reveal it
  if (webviewPanel) {
    webviewPanel.reveal(vscode.ViewColumn.One);
    return;
  }

  // Create new panel
  webviewPanel = vscode.window.createWebviewPanel(
    'spireChat',
    'Spire Chat',
    vscode.ViewColumn.One,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [
        vscode.Uri.file(path.join(context.extensionPath, 'src', 'webview')),
      ],
    }
  );

  // Set HTML content
  webviewPanel.webview.html = getWebviewHtml(webviewPanel.webview, context);

  // Handle messages from webview
  webviewPanel.webview.onDidReceiveMessage(
    (message) => handleWebviewMessage(message as any),
    undefined,
    disposables
  );

  // Handle panel disposal
  webviewPanel.onDidDispose(
    () => {
      webviewPanel = null;
    },
    undefined,
    disposables
  );

  // Notify webview of connection status
  postMessageToAllWebviews({ type: 'status', connected: client?.isRunning ?? false });
}

function getWebviewHtml(webview: vscode.Webview, context: vscode.ExtensionContext): string {
  // Get paths to webview resources
  const styleUri = webview.asWebviewUri(
    vscode.Uri.file(path.join(context.extensionPath, 'src', 'webview', 'style.css'))
  );
  const scriptUri = webview.asWebviewUri(
    vscode.Uri.file(path.join(context.extensionPath, 'src', 'webview', 'app.js'))
  );

  return `<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Spire</title>
  <link rel="stylesheet" href="${styleUri}">
</head>
<body>
  <!-- Tab Navigation Bar -->
  <nav class="tab-bar" id="tab-bar">
    <button class="tab-btn active" data-tab="chat">💬 Chat</button>
    <button class="tab-btn" data-tab="mcp">🔌 MCP</button>
    <button class="tab-btn" data-tab="project">📊 Project</button>
  </nav>

  <!-- Connection Status -->
  <div class="connection-status">
    <span class="status-dot disconnected" id="status-dot"></span>
    <span id="status-text">Disconnected</span>
  </div>

  <!-- Error Banner -->
  <div class="error-banner" id="error-banner"></div>

  <!-- ── Tab: Chat ─────────────────────────────────────────────────────── -->
  <div class="tab-content active" id="tab-chat">
    <!-- Chat Header Bar (top-right settings gear) -->
    <div class="chat-header">
      <button class="settings-btn" id="settings-btn" title="Settings">⚙</button>
    </div>

    <div class="messages" id="messages">
      <div class="empty-state" id="empty-state">
        <div class="empty-state-icon">💬</div>
        <div class="empty-state-text">Start a conversation</div>
        <div class="empty-state-hint">Type a message below to begin chatting with Spire</div>
      </div>
    </div>

    <!-- Typing Indicator (hidden by default) -->
    <div class="typing-indicator hidden" id="typing-indicator">
      <span class="typing-dot"></span>
      <span class="typing-dot"></span>
      <span class="typing-dot"></span>
    </div>

    <!-- Input Area -->
    <div class="input-area">
      <div class="input-wrapper">
        <textarea
          id="message-input"
          placeholder="Type a message..."
          rows="1"
          autofocus
        ></textarea>
      </div>
      <button class="send-btn" id="send-btn" disabled>Send</button>
    </div>

    <!-- Chat Settings Panel (slide-in overlay) -->
    <div class="chat-settings-panel hidden" id="chat-settings-panel">
      <div class="chat-settings-header">
        <span class="chat-settings-title">DeepSeek Configuration</span>
        <button class="chat-settings-close" id="chat-settings-close">✕</button>
      </div>
      <div class="chat-settings-body">
        <div class="config-field">
          <label class="config-label" for="config-api-key">API Key</label>
          <div class="config-password-wrapper">
            <input type="password" id="config-api-key" class="config-input config-password-input" placeholder="sk-..." autocomplete="off" />
            <button class="config-toggle-btn" id="config-toggle-key" title="Show/hide API key">👁</button>
          </div>
          <span class="config-hint">Your DeepSeek API key (stored securely in the graph database)</span>
        </div>

        <div class="config-field">
          <label class="config-label" for="config-model">Model</label>
          <select id="config-model" class="config-input config-select">
            <option value="deepseek-v4-pro">deepseek-v4-pro</option>
            <option value="deepseek-v4-flash">deepseek-v4-flash</option>
          </select>
          <span class="config-hint">The DeepSeek model to use for completions</span>
        </div>

        <div class="config-field">
          <label class="config-label" for="config-api-url">API URL</label>
          <select id="config-api-url" class="config-input config-select">
            <option value="https://api.deepseek.com/v1/chat/completions">https://api.deepseek.com/v1/chat/completions</option>
            <option value="https://api.deepseek.com/beta/chat/completions">https://api.deepseek.com/beta/chat/completions</option>
          </select>
          <span class="config-hint">The DeepSeek API endpoint URL</span>
        </div>

        <div class="config-actions">
          <button class="config-btn config-btn-primary" id="config-save-btn">Save Configuration</button>
          <span class="config-status" id="config-status"></span>
        </div>
      </div>
    </div>
  </div>

  <!-- ── Tab: MCP ──────────────────────────────────────────────────────── -->
  <div class="tab-content" id="tab-mcp">
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">MCP Servers</span>
      <button class="header-btn" id="mcp-refresh-btn" title="Refresh MCP servers">⟳ Refresh</button>
    </div>
    <div class="mcp-server-list" id="mcp-server-list">
      <div class="empty-state" id="mcp-empty-state">
        <div class="empty-state-icon">🔌</div>
        <div class="empty-state-text">No MCP servers</div>
        <div class="empty-state-hint">MCP servers will appear here when connected</div>
      </div>
    </div>
  </div>

  <!-- ── Tab: Project (read-only project analysis) ────────────────────── -->
  <div class="tab-content" id="tab-project">
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">Project Analysis</span>
      <div style="display:flex;gap:4px">
        <button class="header-btn" id="project-refresh-btn" title="Refresh project analysis">⟳ Refresh</button>
      </div>
    </div>
    <div class="project-content" id="project-content">
      <div class="empty-state" id="project-empty-state">
        <div class="empty-state-icon">📊</div>
        <div class="empty-state-text">Project analysis</div>
        <div class="empty-state-hint">Loading project overview...</div>
      </div>
    </div>
  </div>

  <script src="${scriptUri}"></script>
</body>
</html>`;
}

// ── Message Proxy ────────────────────────────────────────────────────────────

interface WebviewCallMessage {
  type: 'call';
  method: string;
  params: unknown;
  id: number;
}

interface WebviewNotifyMessage {
  type: 'notify';
  method: string;
  params: unknown;
}

interface WebviewReadyMessage {
  type: 'webviewReady';
}

type WebviewMessage = WebviewCallMessage | WebviewNotifyMessage | WebviewReadyMessage;

async function handleWebviewMessage(message: WebviewMessage): Promise<void> {
  switch (message.type) {
    case 'call':
      await handleCall(message);
      break;

    case 'notify':
      handleNotify(message);
      break;

    case 'webviewReady':
      webviewReady = true;
      postMessageToAllWebviews({ type: 'status', connected: client?.isRunning ?? false });
      break;

    default:
      logger.warn(`Unknown webview message type: ${(message as any).type}`);
  }
}

async function handleCall(msg: WebviewCallMessage): Promise<void> {
  // Route the call to the subprocess (Rust core) for application-level methods
  // or to the local router for VS Code API methods
  const isVscodeMethod = msg.method.startsWith('workspace/') ||
    msg.method.startsWith('editor/') ||
    msg.method.startsWith('diagnostics/') ||
    msg.method.startsWith('terminal/') ||
    msg.method.startsWith('git/') ||
    msg.method.startsWith('symbols/') ||
    msg.method.startsWith('document/');

  // Methods that should always go to the subprocess (Rust core / mock server)
  const isSubprocessMethod = msg.method.startsWith('mcp/') ||
    msg.method.startsWith('tools/') ||
    msg.method.startsWith('chat/') ||
    msg.method.startsWith('agent/') ||
    msg.method.startsWith('llm/') ||
    msg.method.startsWith('config/') ||
    msg.method.startsWith('memory_graph/');


  try {
    let result: unknown;

    if (isSubprocessMethod && client && client.isRunning) {
      // Forward to subprocess (Rust core / mock server)
      result = await client.call(msg.method, msg.params);
    } else if (isVscodeMethod && localRouter) {
      // Handle VS Code API methods locally
      const request = {
        jsonrpc: '2.0' as const,
        id: msg.id,
        method: msg.method,
        params: msg.params,
      };
      result = await localRouter.handle(request);
    } else if (client && client.isRunning) {
      // Default: forward to subprocess
      result = await client.call(msg.method, msg.params);
    } else if (localRouter) {
      // Fallback to local router
      const request = {
        jsonrpc: '2.0' as const,
        id: msg.id,
        method: msg.method,
        params: msg.params,
      };
      result = await localRouter.handle(request);
    } else {
      throw new Error('No router or subprocess available');
    }

    postMessageToAllWebviews({
      type: 'response',
      id: msg.id,
      result,
    });
  } catch (err) {
    const errorObj = err as { code?: number; message?: string };
    postMessageToAllWebviews({
      type: 'response',
      id: msg.id,
      error: errorObj.message ?? String(err),
      code: errorObj.code ?? -32603,
    });
  }
}

function handleNotify(msg: WebviewNotifyMessage): void {
  // Forward notifications to the subprocess
  if (client && client.isRunning) {
    client.notify(msg.method, msg.params);
  } else if (localRouter) {
    // Fallback to local router
    const notification = {
      jsonrpc: '2.0' as const,
      method: msg.method,
      params: msg.params,
    };
    localRouter.handle(notification as any).catch((err) => {
      logger.error(`Error handling notification ${msg.method}:`, err);
    });
  }
}

/**
 * Post a message to all active webviews (sidebar + optional tab panel).
 */
function postMessageToAllWebviews(message: Record<string, unknown>): void {
  // Send to sidebar provider
  if (sidebarProvider) {
    sidebarProvider.postMessage(message);
  }

  // Send to tab panel (if open)
  if (webviewPanel) {
    webviewPanel.webview.postMessage(message);
  }
}
