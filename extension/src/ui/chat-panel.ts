import * as vscode from 'vscode';

export class ChatPanel implements vscode.Disposable {
    public static readonly viewType = 'spire.chat';
    private panel: vscode.WebviewPanel | undefined;

    constructor(private readonly extensionUri: vscode.Uri) {
        // Panel is created lazily on first command
    }

    show(): void {
        if (this.panel) {
            this.panel.reveal(vscode.ViewColumn.Beside);
            return;
        }

        this.panel = vscode.window.createWebviewPanel(
            ChatPanel.viewType,
            'Spire Chat',
            vscode.ViewColumn.Beside,
            {
                enableScripts: true,
                retainContextWhenHidden: true,
                localResourceRoots: [vscode.Uri.joinPath(this.extensionUri, 'media')],
            }
        );

        this.panel.webview.html = this.getHtmlContent();

        this.panel.onDidDispose(() => {
            this.panel = undefined;
        });

        // Handle messages from the webview
        this.panel.webview.onDidReceiveMessage((message) => {
            if (message.type === 'ready') {
                this.showMessage('Spire is ready. Use commands like **Explain Code** or **Search Codebase** to get started.');
            }
        });
    }

    showMessage(content: string): void {
        if (!this.panel) return;
        this.panel.webview.postMessage({
            type: 'message',
            content,
        });
    }

    showProgress(update: { message: string; percent?: number }): void {
        if (!this.panel) return;
        this.panel.webview.postMessage({
            type: 'progress',
            message: update.message,
            percent: update.percent ?? 0,
        });
    }

    dispose(): void {
        this.panel?.dispose();
    }

    private getHtmlContent(): string {
        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Spire Chat</title>
    <style>
        body {
            font-family: var(--vscode-font-family);
            font-size: var(--vscode-font-size);
            color: var(--vscode-editor-foreground);
            background-color: var(--vscode-editor-background);
            padding: 16px;
            margin: 0;
        }
        #messages {
            display: flex;
            flex-direction: column;
            gap: 12px;
        }
        .message {
            padding: 8px 12px;
            border-radius: 6px;
            background-color: var(--vscode-textBlockQuote-background);
            border-left: 3px solid var(--vscode-textLink-foreground);
            line-height: 1.5;
        }
        .progress-bar {
            width: 100%;
            height: 4px;
            background-color: var(--vscode-progressBar-background);
            border-radius: 2px;
            margin-top: 8px;
            overflow: hidden;
        }
        .progress-fill {
            height: 100%;
            background-color: var(--vscode-progressBar-foreground);
            transition: width 0.3s ease;
        }
        .progress-text {
            font-size: 0.9em;
            color: var(--vscode-descriptionForeground);
            margin-top: 4px;
        }
        .status {
            font-size: 0.85em;
            color: var(--vscode-descriptionForeground);
            font-style: italic;
        }
    </style>
</head>
<body>
    <div id="messages">
        <div class="status">Connecting to Spire...</div>
    </div>
    <script>
        (function() {
            const vscode = acquireVsCodeApi();
            const messagesContainer = document.getElementById('messages');

            // Signal that the webview is ready
            vscode.postMessage({ type: 'ready' });

            window.addEventListener('message', (event) => {
                const data = event.data;

                if (data.type === 'message') {
                    const msgDiv = document.createElement('div');
                    msgDiv.className = 'message';
                    msgDiv.innerHTML = data.content;
                    messagesContainer.appendChild(msgDiv);
                    messagesContainer.scrollTop = messagesContainer.scrollHeight;
                } else if (data.type === 'progress') {
                    // Update or create progress indicator
                    let progressDiv = document.getElementById('progress-indicator');
                    if (!progressDiv) {
                        progressDiv = document.createElement('div');
                        progressDiv.id = 'progress-indicator';
                        progressDiv.innerHTML = \`
                            <div class="progress-text"></div>
                            <div class="progress-bar"><div class="progress-fill"></div></div>
                        \`;
                        messagesContainer.appendChild(progressDiv);
                    }
                    progressDiv.querySelector('.progress-text').textContent = data.message;
                    progressDiv.querySelector('.progress-fill').style.width = Math.min(data.percent, 100) + '%';
                }
            });
        })();
    </script>
</body>
</html>`;
    }
}
