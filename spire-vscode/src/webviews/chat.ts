import * as vscode from 'vscode';
import { ChatService, ChatChunkEvent, ProgressEvent, CompleteEvent, ErrorEvent } from '../services/chat';

export class ChatWebViewProvider {
  private panel: vscode.WebviewPanel | null = null;
  private chatService: ChatService;
  private disposables: vscode.Disposable[] = [];
  private messageBuffer: string = '';
  private progressMessage: string = '';
  private isProcessing: boolean = false;

  constructor(chatService: ChatService) {
    this.chatService = chatService;

    // Register event handlers
    this.chatService.onChunk((event: ChatChunkEvent) => {
      this.handleChunk(event);
    });

    this.chatService.onProgress((event: ProgressEvent) => {
      this.handleProgress(event);
    });

    this.chatService.onComplete((event: CompleteEvent) => {
      this.handleComplete(event);
    });

    this.chatService.onError((event: ErrorEvent) => {
      this.handleError(event);
    });
  }

  show(): void {
    if (this.panel) {
      this.panel.reveal(vscode.ViewColumn.One);
      return;
    }

    this.panel = vscode.window.createWebviewPanel(
      'spireChat',
      'Spire Assistant',
      vscode.ViewColumn.One,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
      }
    );

    this.panel.webview.html = this.getHtml();
    this.panel.onDidDispose(() => {
      this.panel = null;
      this.disposables.forEach(d => d.dispose());
      this.disposables = [];
    });

    // Handle messages from webview
    this.panel.webview.onDidReceiveMessage(
      (message) => {
        switch (message.type) {
          case 'sendMessage':
            this.sendMessage(message.text);
            break;
          case 'newSession':
            this.newSession();
            break;
          case 'clearMessages':
            this.clearMessages();
            break;
        }
      },
      undefined,
      this.disposables
    );

    // Send initial state
    this.sendMessageToWebview({ type: 'ready' });
  }

  private sendMessage(text: string): void {
    if (!text.trim()) return;

    this.isProcessing = true;
    this.messageBuffer = '';

    // Add user message immediately
    this.sendMessageToWebview({
      type: 'userMessage',
      text: text,
    });

    // Clear previous progress
    this.progressMessage = '';
    this.sendMessageToWebview({
      type: 'progress',
      message: 'Thinking...',
    });

    // Get context from active editor
    const context: any = {};
    const editor = vscode.window.activeTextEditor;
    if (editor) {
      context.filePath = editor.document.uri.fsPath;
      context.projectRoot = vscode.workspace.getWorkspaceFolder(editor.document.uri)?.uri.fsPath;
      const selection = editor.selection;
      if (!selection.isEmpty) {
        context.selection = editor.document.getText(selection);
      }
    }

    this.chatService.sendStreamingMessage(text, context).catch((err) => {
      this.sendMessageToWebview({
        type: 'error',
        error: err.message,
      });
      this.isProcessing = false;
    });
  }

  private newSession(): void {
    this.chatService.newSession();
    this.clearMessages();
    this.sendMessageToWebview({
      type: 'sessionChanged',
      sessionId: this.chatService.getSessionId(),
    });
  }

  private clearMessages(): void {
    this.messageBuffer = '';
    this.progressMessage = '';
    this.isProcessing = false;
    this.sendMessageToWebview({ type: 'clearMessages' });
  }

  private handleChunk(event: ChatChunkEvent): void {
    if (event.done) {
      // Final chunk, message complete
      this.sendMessageToWebview({
        type: 'assistantMessage',
        text: this.messageBuffer,
        done: true,
      });
      this.isProcessing = false;
      this.progressMessage = '';
      this.sendMessageToWebview({
        type: 'progress',
        message: '✅ Done',
        done: true,
      });
    } else {
      this.messageBuffer += event.chunk;
      this.sendMessageToWebview({
        type: 'assistantMessage',
        text: this.messageBuffer,
        done: false,
      });
    }
  }

  private handleProgress(event: ProgressEvent): void {
    this.progressMessage = `${event.message} (${event.step}/${event.total})`;
    this.sendMessageToWebview({
      type: 'progress',
      message: this.progressMessage,
      step: event.step,
      total: event.total,
    });
  }

  private handleComplete(event: CompleteEvent): void {
    this.isProcessing = false;
    this.progressMessage = '✅ Complete';
    this.sendMessageToWebview({
      type: 'progress',
      message: this.progressMessage,
      done: true,
    });
    if (event.artifacts.length > 0) {
      this.sendMessageToWebview({
        type: 'artifacts',
        artifacts: event.artifacts,
      });
    }
  }

  private handleError(event: ErrorEvent): void {
    this.isProcessing = false;
    this.progressMessage = '❌ Error';
    this.sendMessageToWebview({
      type: 'progress',
      message: this.progressMessage,
      done: true,
    });
    this.sendMessageToWebview({
      type: 'error',
      error: event.error,
      suggestion: event.suggestion,
    });
  }

  private sendMessageToWebview(message: any): void {
    if (this.panel) {
      this.panel.webview.postMessage(message);
    }
  }

  private getHtml(): string {
    return `
      <!DOCTYPE html>
      <html>
      <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0">
        <title>Spire Assistant</title>
        <style>
          * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
          }
          body {
            font-family: var(--vscode-font-family);
            background: var(--vscode-sideBar-background);
            color: var(--vscode-foreground);
            height: 100vh;
            display: flex;
            flex-direction: column;
            padding: 10px;
          }
          #header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 10px;
            padding-bottom: 10px;
            border-bottom: 1px solid var(--vscode-panel-border);
          }
          #header h2 {
            font-size: 14px;
            font-weight: 600;
            color: var(--vscode-titleBar-activeForeground);
          }
          #header-actions button {
            background: none;
            border: none;
            color: var(--vscode-foreground);
            cursor: pointer;
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 12px;
          }
          #header-actions button:hover {
            background: var(--vscode-toolbar-hoverBackground);
          }
          #messages {
            flex: 1;
            overflow-y: auto;
            padding: 10px 0;
          }
          .message {
            margin: 8px 0;
            padding: 10px 12px;
            border-radius: 8px;
            max-width: 90%;
            word-wrap: break-word;
            white-space: pre-wrap;
          }
          .message-user {
            background: var(--vscode-input-background);
            align-self: flex-end;
            margin-left: auto;
            border-bottom-right-radius: 4px;
          }
          .message-assistant {
            background: var(--vscode-editor-selectionBackground);
            align-self: flex-start;
            border-bottom-left-radius: 4px;
          }
          .message-assistant.typing {
            opacity: 0.7;
          }
          #progress-container {
            padding: 8px 0;
            font-size: 12px;
            color: var(--vscode-descriptionForeground);
            min-height: 28px;
          }
          #progress-container .spinner {
            display: inline-block;
            animation: spin 1s linear infinite;
          }
          @keyframes spin {
            from { transform: rotate(0deg); }
            to { transform: rotate(360deg); }
          }
          #input-container {
            display: flex;
            gap: 8px;
            padding: 10px 0;
            border-top: 1px solid var(--vscode-panel-border);
          }
          #input {
            flex: 1;
            padding: 8px 12px;
            background: var(--vscode-input-background);
            color: var(--vscode-input-foreground);
            border: 1px solid var(--vscode-input-border);
            border-radius: 4px;
            font-family: var(--vscode-font-family);
            font-size: 13px;
            resize: none;
            outline: none;
          }
          #input:focus {
            border-color: var(--vscode-focusBorder);
          }
          #send-button {
            padding: 8px 16px;
            background: var(--vscode-button-background);
            color: var(--vscode-button-foreground);
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-family: var(--vscode-font-family);
            font-size: 13px;
          }
          #send-button:hover {
            background: var(--vscode-button-hoverBackground);
          }
          #send-button:disabled {
            opacity: 0.5;
            cursor: not-allowed;
          }
          .error {
            color: var(--vscode-errorForeground);
            padding: 8px 12px;
            background: var(--vscode-inputValidation-errorBackground);
            border-radius: 4px;
            margin: 4px 0;
          }
          .suggestion {
            color: var(--vscode-terminal-ansiBrightBlue);
            padding: 4px 12px 8px 12px;
            font-size: 12px;
          }
          .artifact {
            font-size: 12px;
            color: var(--vscode-textLink-foreground);
            cursor: pointer;
            padding: 2px 0;
          }
          .artifact:hover {
            text-decoration: underline;
          }
          #empty-state {
            display: flex;
            flex-direction: column;
            align-items: center;
            justify-content: center;
            height: 100%;
            color: var(--vscode-descriptionForeground);
            text-align: center;
          }
          #empty-state .icon {
            font-size: 48px;
            margin-bottom: 16px;
          }
          #empty-state h3 {
            font-weight: 400;
          }
          .hidden {
            display: none !important;
          }
        </style>
      </head>
      <body>
        <div id="header">
          <h2>🤖 Spire Assistant</h2>
          <div id="header-actions">
            <button id="new-session-btn">New Session</button>
            <button id="clear-btn">Clear</button>
          </div>
        </div>
        <div id="messages">
          <div id="empty-state">
            <div class="icon">🤖</div>
            <h3>Ask Spire anything about your code</h3>
            <p style="font-size:13px; margin-top:8px;">Try: "Build the project" or "Explain this function"</p>
          </div>
        </div>
        <div id="progress-container"></div>
        <div id="input-container">
          <textarea id="input" rows="1" placeholder="Ask Spire... (Ctrl+Enter to send)" disabled></textarea>
          <button id="send-button" disabled>Send</button>
        </div>

        <script>
          const vscode = acquireVsCodeApi();

          // DOM Elements
          const messagesContainer = document.getElementById('messages');
          const emptyState = document.getElementById('empty-state');
          const input = document.getElementById('input');
          const sendButton = document.getElementById('send-button');
          const progressContainer = document.getElementById('progress-container');
          const newSessionBtn = document.getElementById('new-session-btn');
          const clearBtn = document.getElementById('clear-btn');

          let isProcessing = false;
          let currentAssistantMessage = null;
          let messageBuffer = '';
          let messageCounter = 0;

          // Auto-resize textarea
          input.addEventListener('input', () => {
            input.style.height = 'auto';
            input.style.height = Math.min(input.scrollHeight, 100) + 'px';
          });

          // Send on Ctrl+Enter
          input.addEventListener('keydown', (e) => {
            if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
              e.preventDefault();
              sendMessage();
            }
            if (e.key === 'Enter' && !e.shiftKey) {
              e.preventDefault();
              sendMessage();
            }
          });

          sendButton.addEventListener('click', sendMessage);

          newSessionBtn.addEventListener('click', () => {
            vscode.postMessage({ type: 'newSession' });
          });

          clearBtn.addEventListener('click', () => {
            vscode.postMessage({ type: 'clearMessages' });
          });

          function sendMessage() {
            const text = input.value.trim();
            if (!text || isProcessing) return;

            vscode.postMessage({
              type: 'sendMessage',
              text: text,
            });

            input.value = '';
            input.style.height = 'auto';
            isProcessing = true;
            sendButton.disabled = true;
            input.disabled = true;
          }

          function addMessage(text, role) {
            const div = document.createElement('div');
            div.className = 'message message-' + role;
            div.textContent = text;
            div.setAttribute('data-id', messageCounter++);
            messagesContainer.appendChild(div);
            scrollToBottom();

            // Hide empty state
            emptyState.classList.add('hidden');
          }

          function updateLastMessage(text) {
            const messages = messagesContainer.querySelectorAll('.message');
            const last = messages[messages.length - 1];
            if (last && last.classList.contains('message-assistant')) {
              last.textContent = text;
            } else {
              addMessage(text, 'assistant');
            }
            scrollToBottom();
          }

          function addAssistantMessage(text, done) {
            const div = document.createElement('div');
            div.className = 'message message-assistant';
            if (!done) div.classList.add('typing');
            div.textContent = text;
            div.setAttribute('data-id', messageCounter++);
            messagesContainer.appendChild(div);
            currentAssistantMessage = div;
            scrollToBottom();
            emptyState.classList.add('hidden');
          }

          function scrollToBottom() {
            messagesContainer.scrollTop = messagesContainer.scrollHeight;
          }

          function setProgress(message, step, total, done) {
            if (done) {
              progressContainer.textContent = message || '';
              return;
            }
            let text = message || '';
            if (step !== undefined && total !== undefined) {
              text += ' (' + step + '/' + total + ')';
            }
            progressContainer.textContent = text;
          }

          function showError(error, suggestion) {
            const div = document.createElement('div');
            div.className = 'error';
            div.textContent = '❌ ' + error;
            messagesContainer.appendChild(div);
            if (suggestion) {
              const suggestionDiv = document.createElement('div');
              suggestionDiv.className = 'suggestion';
              suggestionDiv.textContent = '💡 ' + suggestion;
              messagesContainer.appendChild(suggestionDiv);
            }
            scrollToBottom();
          }

          function showArtifacts(artifacts) {
            for (const artifact of artifacts) {
              const div = document.createElement('div');
              div.className = 'artifact';
              div.textContent = '📎 ' + artifact;
              div.addEventListener('click', () => {
                vscode.postMessage({ type: 'openArtifact', path: artifact });
              });
              messagesContainer.appendChild(div);
            }
            scrollToBottom();
          }

          function clearMessages() {
            messagesContainer.innerHTML = '';
            emptyState.classList.remove('hidden');
            progressContainer.textContent = '';
            currentAssistantMessage = null;
            messageBuffer = '';
            isProcessing = false;
            sendButton.disabled = false;
            input.disabled = false;
            input.focus();
          }

          // Handle messages from extension
          window.addEventListener('message', (event) => {
            const msg = event.data;

            switch (msg.type) {
              case 'ready':
                input.disabled = false;
                sendButton.disabled = false;
                input.focus();
                break;

              case 'userMessage':
                addMessage(msg.text, 'user');
                break;

              case 'assistantMessage':
                if (!msg.done) {
                  messageBuffer = msg.text;
                  // Check if we already have an assistant message
                  const last = messagesContainer.querySelector('.message:last-child');
                  if (last && last.classList.contains('message-assistant')) {
                    last.textContent = msg.text;
                    last.classList.remove('typing');
                  } else {
                    addAssistantMessage(msg.text, false);
                  }
                } else {
                  messageBuffer = msg.text;
                  // Update the last message to show it's complete
                  const last = messagesContainer.querySelector('.message:last-child');
                  if (last && last.classList.contains('message-assistant')) {
                    last.textContent = msg.text;
                    last.classList.remove('typing');
                  } else {
                    addAssistantMessage(msg.text, true);
                  }
                }
                scrollToBottom();
                break;

              case 'progress':
                setProgress(msg.message, msg.step, msg.total, msg.done);
                break;

              case 'error':
                showError(msg.error, msg.suggestion);
                isProcessing = false;
                sendButton.disabled = false;
                input.disabled = false;
                input.focus();
                break;

              case 'artifacts':
                showArtifacts(msg.artifacts);
                break;

              case 'clearMessages':
                clearMessages();
                break;

              case 'sessionChanged':
                clearMessages();
                setProgress('New session: ' + msg.sessionId.slice(0, 12));
                break;
            }
          });
        </script>
      </body>
      </html>
    `;
  }
}
