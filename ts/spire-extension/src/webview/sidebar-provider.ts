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
  <!-- Startup Overlay (visible during initialization) -->
  <div class="startup-overlay" id="startup-overlay">
    <div class="startup-card">
      <div class="startup-spinner-container">
        <div class="startup-spinner"></div>
        <div class="startup-logo">Spire</div>
        <div class="startup-status" id="startup-status">Initializing...</div>
      </div>
    </div>
  </div>

  <!-- Tab Navigation Bar -->
  <nav class="tab-bar" id="tab-bar">
    <button class="tab-btn active" data-tab="chat">💬 Chat</button>
    <button class="tab-btn" data-tab="mcp">🔌 MCP</button>
    <button class="tab-btn" data-tab="tools">🛠 Tools</button>
    <button class="tab-btn" data-tab="agents">🤖 Agents</button>
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
      <button class="settings-btn" id="settings-btn" title="Settings">⚙</button>
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
          <option value="deepseek-chat">deepseek-chat</option>
          <option value="deepseek-coder">deepseek-coder</option>
          <option value="deepseek-reasoner">deepseek-reasoner</option>
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
        <button class="header-btn" id="mcp-add-btn" title="Add new MCP server">✚ Add</button>
        <button class="header-btn" id="mcp-refresh-btn" title="Refresh MCP servers">⟳ Refresh</button>
        <button class="header-btn" id="mcp-import-btn" title="Import MCP config from JSON file">📂 Import</button>
      </div>
    </div>
    <div class="mcp-server-list" id="mcp-server-list">
      <div class="empty-state" id="mcp-empty-state">
        <div class="empty-state-icon">🔌</div>
        <div class="empty-state-text">No MCP servers</div>
        <div class="empty-state-hint">MCP servers will appear here when connected</div>
      </div>
    </div>

    <!-- ── MCP Config Editor Modal ── -->
    <div class="modal-overlay hidden" id="mcp-config-modal">
      <div class="modal-content">
        <div class="modal-header">
          <span class="modal-title" id="mcp-config-modal-title">Edit MCP Server</span>
          <button class="modal-close-btn" id="mcp-config-modal-close">✕</button>
        </div>
        <div class="modal-body">
          <div class="config-field">
            <label class="config-label">Name *</label>
            <input class="config-input" id="mcp-config-name" placeholder="e.g. my-filesystem-server" />
          </div>
          <div class="config-field">
            <label class="config-label">Transport Type</label>
            <select class="config-input config-select" id="mcp-config-transport">
              <option value="stdio">stdio (command + args)</option>
              <option value="http">HTTP (URL + headers)</option>
            </select>
          </div>
          <div class="config-field" id="mcp-config-command-group">
            <label class="config-label">Command</label>
            <input class="config-input" id="mcp-config-command" placeholder="e.g. npx, uvx, node" />
          </div>
          <div class="config-field" id="mcp-config-args-group">
            <label class="config-label">Arguments (one per line)</label>
            <textarea class="config-input config-textarea" id="mcp-config-args" rows="3" placeholder="e.g. -y&#10;@modelcontextprotocol/server-filesystem&#10;/path/to/dir"></textarea>
          </div>
          <div class="config-field" id="mcp-config-url-group" style="display:none">
            <label class="config-label">URL</label>
            <input class="config-input" id="mcp-config-url" placeholder="e.g. http://localhost:3000/mcp" />
          </div>
          <div class="config-field" id="mcp-config-headers-group" style="display:none">
            <label class="config-label">Headers (JSON object)</label>
            <textarea class="config-input config-textarea" id="mcp-config-headers" rows="2" placeholder='e.g. {"Authorization": "Bearer token"}'></textarea>
          </div>
          <div class="config-field">
            <label class="config-label">Environment Variables (JSON object)</label>
            <textarea class="config-input config-textarea" id="mcp-config-env" rows="2" placeholder='e.g. {"API_KEY": "value"}'></textarea>
          </div>
          <div class="config-field config-checkbox-field">
            <label class="config-checkbox-label">
              <input type="checkbox" id="mcp-config-autostart" checked />
              <span>Auto-start on connection</span>
            </label>
          </div>
        </div>
        <div class="modal-footer">
          <div class="modal-footer-left">
            <button class="header-btn danger-btn hidden" id="mcp-config-delete-btn" title="Delete this MCP server">🗑 Delete</button>
          </div>
          <div class="modal-footer-right">
            <span class="config-status" id="mcp-config-status"></span>
            <button class="header-btn" id="mcp-config-cancel-btn">Cancel</button>
            <button class="config-btn config-btn-primary" id="mcp-config-save-btn">Save</button>
          </div>
        </div>
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

  <!-- ── Tab: Agents ───────────────────────────────────────────────────── -->
  <div class="tab-content" id="tab-agents">
    <div class="placeholder-tab">
      <div class="placeholder-icon">🤖</div>
      <div class="placeholder-text">Agents</div>
      <div class="placeholder-hint">Not yet implemented</div>
    </div>
  </div>

  <script src="${scriptUri}"></script>
</body>
</html>`;
  }
}
