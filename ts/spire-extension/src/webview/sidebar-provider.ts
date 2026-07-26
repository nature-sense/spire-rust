import * as vscode from 'vscode';
import * as path from 'path';
import { logger } from '../util/logger';

/**
 * Spire Chat Sidebar Provider — implements WebviewViewProvider for the sidebar.
 *
 * This provider creates and manages the webview shown in the VS Code sidebar
 * (spire-sidebar container → spire.chatView). It handles:
 *   - Creating the webview with the chat HTML/JS/CSS
 *   - Forwarding messages from the webview to the extension host
 *   - Forwarding messages from the extension host to the webview
 */
export class ChatSidebarProvider implements vscode.WebviewViewProvider {
  public static readonly viewType = 'spire.chatView';

  private _view?: vscode.WebviewView;
  private _messageHandler?: (message: Record<string, unknown>) => Promise<void>;

  constructor(private readonly _extensionUri: vscode.Uri) {}

  /**
   * Set the handler for messages coming FROM the webview.
   * Called by the extension to wire up message routing.
   */
  setMessageHandler(handler: (message: Record<string, unknown>) => Promise<void>): void {
    this._messageHandler = handler;
  }

  /**
   * Post a message TO the webview.
   * Called by the extension to forward status updates, notifications, etc.
   */
  postMessage(message: Record<string, unknown>): void {
    if (this._view) {
      this._view.webview.postMessage(message);
    }
  }

  /**
   * Called by VS Code when the webview view is first created or revealed.
   */
  resolveWebviewView(
    webviewView: vscode.WebviewView,
    _context: vscode.WebviewViewResolveContext,
    _token: vscode.CancellationToken
  ): void {
    this._view = webviewView;

    webviewView.webview.options = {
      enableScripts: true,
      localResourceRoots: [
        vscode.Uri.file(path.join(this._extensionUri.fsPath, 'src', 'webview')),
      ],
    };

    webviewView.webview.html = this.getHtmlForWebview(webviewView.webview);

    // Handle messages from the webview
    webviewView.webview.onDidReceiveMessage(
      (message: Record<string, unknown>) => {
        if (this._messageHandler) {
          this._messageHandler(message).catch((err) => {
            logger.error(`Error handling webview message: ${err}`);
          });
        }
      }
    );

    // When the view becomes visible, notify the extension
    webviewView.onDidChangeVisibility(() => {
      if (webviewView.visible) {
        logger.debug('Chat sidebar became visible');
      }
    });

    logger.info('Chat sidebar webview resolved');
  }

  /**
   * Get the HTML content for a webview.
   * Public so it can be reused for fallback webview panels.
   */
  getHtmlForWebview(webview: vscode.Webview): string {
    const styleUri = webview.asWebviewUri(
      vscode.Uri.file(path.join(this._extensionUri.fsPath, 'src', 'webview', 'style.css'))
    );
    const scriptUri = webview.asWebviewUri(
      vscode.Uri.file(path.join(this._extensionUri.fsPath, 'src', 'webview', 'app.js'))
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
    <button class="tab-btn" data-tab="tools">🛠 Tools</button>
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
    <!-- Chat Toolbar -->
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">Chat</span>
      <div style="display:flex;gap:4px">
        <button class="header-btn" id="clear-btn" title="Clear conversation">🗑 Clear</button>
        <button class="header-btn" id="settings-btn" title="Settings">⚙️ Settings</button>
        <button class="header-btn" id="new-chat-btn" title="New chat">✚ New</button>
      </div>
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
  </div>

  <!-- ── Chat Settings Panel (slide-in overlay within chat tab) ────────── -->
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

  <!-- ── Tab: MCP ──────────────────────────────────────────────────────── -->
  <div class="tab-content" id="tab-mcp">
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">MCP Servers</span>
      <div style="display:flex;gap:4px">
        <button class="header-btn" id="mcp-refresh-btn" title="Refresh MCP servers">⟳ Refresh</button>
      </div>
    </div>
    <div class="mcp-server-list" id="mcp-server-list">
      <div class="empty-state" id="mcp-empty-state">
        <div class="empty-state-icon">🔌</div>
        <div class="empty-state-text">No MCP servers</div>
        <div class="empty-state-hint">MCP servers will appear here when connected</div>
      </div>
    </div>
  </div>

  <!-- ── Tab: Tools (real-time tool usage) ────────────────────────────── -->
  <div class="tab-content" id="tab-tools">
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">Tool Activity</span>
      <button class="header-btn" id="tools-clear-btn" title="Clear tool log">🗑 Clear</button>
    </div>
    <div class="tools-feed" id="tools-feed">
      <div class="empty-state" id="tools-empty-state">
        <div class="empty-state-icon">🛠</div>
        <div class="empty-state-text">No tool activity yet</div>
        <div class="empty-state-hint">Tools called by the AI will appear here in real-time</div>
      </div>
    </div>
  </div>

  <!-- ── Tab: Project (graphical project overview) ────────────────────── -->
  <div class="tab-content" id="tab-project">
    <div class="tab-toolbar">
      <span class="tab-toolbar-title">Project Analysis</span>
      <div style="display:flex;gap:4px">
        <button class="header-btn active-view" id="project-overview-btn" title="Text overview">📋 Overview</button>
        <button class="header-btn" id="project-graph-btn" title="Graph view">🔗 Graph</button>
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
    <div class="project-graph hidden" id="project-graph">
      <div class="graph-legend">
        <span class="legend-item"><span class="legend-dot" style="background:#4a90d9"></span> Source</span>
        <span class="legend-item"><span class="legend-dot" style="background:#50b86c"></span> Tests</span>
        <span class="legend-item"><span class="legend-dot" style="background:#e6a817"></span> Docs</span>
        <span class="legend-item"><span class="legend-dot" style="background:#9b59b6"></span> Build</span>
        <span class="legend-item"><span class="legend-dot" style="background:#e74c3c"></span> Config</span>
        <span class="legend-item"><span class="legend-dot" style="background:#95a5a6"></span> Other</span>
      </div>
      <div id="cy"></div>
    </div>
  </div>

  <script src="https://unpkg.com/cytoscape@3.30.4/dist/cytoscape.min.js"></script>
  <script src="https://unpkg.com/dagre@0.8.5/dist/dagre.min.js"></script>
  <script src="https://unpkg.com/cytoscape-dagre@2.5.0/cytoscape-dagre.js"></script>
  <script src="${scriptUri}"></script>
</body>
</html>`;
  }
}
