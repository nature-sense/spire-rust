import * as vscode from 'vscode';
import { ConfigService } from '../services/config';

export class ConfigWebViewProvider {
  private panel: vscode.WebviewPanel | null = null;
  private configService: ConfigService;
  private config: Record<string, any> = {};
  private disposables: vscode.Disposable[] = [];

  constructor(configService: ConfigService) {
    this.configService = configService;

    this.configService.onConfigChanged((event) => {
      this.handleConfigChanged(event);
    });
  }

  show(): void {
    if (this.panel) {
      this.panel.reveal(vscode.ViewColumn.Two);
      return;
    }

    this.panel = vscode.window.createWebviewPanel(
      'spireConfig',
      'Spire Configuration',
      vscode.ViewColumn.Two,
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
          case 'loadConfig':
            this.loadConfig();
            break;
          case 'saveConfig':
            this.saveConfig(message.values);
            break;
          case 'runAgent':
            this.runAgent(message.agent, message.goal);
            break;
          case 'getAgentStatus':
            this.getAgentStatus(message.agent);
            break;
        }
      },
      undefined,
      this.disposables
    );

    // Load config immediately
    this.loadConfig();
  }

  private async loadConfig(): Promise<void> {
    try {
      const config = await this.configService.getConfig();
      this.config = config;
      this.sendMessageToWebview({
        type: 'configLoaded',
        config: config,
      });
    } catch (e) {
      this.sendMessageToWebview({
        type: 'error',
        error: e instanceof Error ? e.message : String(e),
      });
    }
  }

  private async saveConfig(values: Record<string, any>): Promise<void> {
    try {
      await this.configService.setConfig(values);
      vscode.window.showInformationMessage('Configuration saved');
      // Reload to get updated values
      this.loadConfig();
    } catch (e) {
      vscode.window.showErrorMessage(`Failed to save config: ${e}`);
    }
  }

  private async runAgent(agent: string, goal: string): Promise<void> {
    try {
      const result = await this.configService.runAgent(agent, goal);
      this.sendMessageToWebview({
        type: 'agentResult',
        result: result,
      });
      vscode.window.showInformationMessage(`Agent ${agent} started`);
    } catch (e) {
      vscode.window.showErrorMessage(`Failed to run agent: ${e}`);
    }
  }

  private async getAgentStatus(agent: string): Promise<void> {
    try {
      const status = await this.configService.getAgentStatus(agent);
      this.sendMessageToWebview({
        type: 'agentStatus',
        agent: agent,
        status: status,
      });
    } catch (e) {
      // Silently fail
    }
  }

  private handleConfigChanged(event: any): void {
    // Update the config in memory
    this.config[event.key] = event.newValue;
    this.sendMessageToWebview({
      type: 'configChanged',
      key: event.key,
      value: event.newValue,
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
        <title>Spire Configuration</title>
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
            padding: 20px;
          }
          h2 {
            font-size: 16px;
            font-weight: 600;
            margin-bottom: 16px;
            color: var(--vscode-titleBar-activeForeground);
          }
          .section {
            margin-bottom: 20px;
            padding: 16px;
            background: var(--vscode-editor-background);
            border-radius: 6px;
            border: 1px solid var(--vscode-panel-border);
          }
          .section-title {
            font-size: 13px;
            font-weight: 600;
            margin-bottom: 12px;
            color: var(--vscode-descriptionForeground);
          }
          .field {
            margin-bottom: 12px;
          }
          .field:last-child {
            margin-bottom: 0;
          }
          .field label {
            display: block;
            font-size: 12px;
            color: var(--vscode-descriptionForeground);
            margin-bottom: 4px;
          }
          .field input, .field select {
            width: 100%;
            padding: 6px 10px;
            background: var(--vscode-input-background);
            color: var(--vscode-input-foreground);
            border: 1px solid var(--vscode-input-border);
            border-radius: 4px;
            font-size: 13px;
            font-family: var(--vscode-font-family);
            outline: none;
          }
          .field input:focus, .field select:focus {
            border-color: var(--vscode-focusBorder);
          }
          .field .description {
            font-size: 11px;
            color: var(--vscode-descriptionForeground);
            margin-top: 4px;
          }
          .button {
            padding: 6px 12px;
            background: var(--vscode-button-background);
            color: var(--vscode-button-foreground);
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 13px;
            font-family: var(--vscode-font-family);
          }
          .button:hover {
            background: var(--vscode-button-hoverBackground);
          }
          .button-secondary {
            background: var(--vscode-button-secondaryBackground);
            color: var(--vscode-button-secondaryForeground);
          }
          .button-secondary:hover {
            background: var(--vscode-button-secondaryHoverBackground);
          }
          .actions {
            display: flex;
            gap: 8px;
            margin-top: 16px;
          }
          .status {
            font-size: 12px;
            color: var(--vscode-descriptionForeground);
            margin-top: 8px;
          }
          .status .running {
            color: var(--vscode-terminal-ansiGreen);
          }
          .status .failed {
            color: var(--vscode-errorForeground);
          }
          .loading {
            opacity: 0.6;
            pointer-events: none;
          }
          .error {
            color: var(--vscode-errorForeground);
            padding: 8px 12px;
            background: var(--vscode-inputValidation-errorBackground);
            border-radius: 4px;
            margin: 8px 0;
          }
          .agent-status {
            font-size: 12px;
            padding: 8px;
            background: var(--vscode-input-background);
            border-radius: 4px;
            margin-top: 8px;
            font-family: var(--vscode-editor-font-family);
          }
          .hidden {
            display: none;
          }
          .agent-row {
            display: flex;
            gap: 8px;
            align-items: center;
            margin-bottom: 8px;
          }
          .agent-row input {
            flex: 1;
          }
        </style>
      </head>
      <body>
        <h2>⚙️ Spire Configuration</h2>

        <div id="error-container" class="hidden"></div>

        <div id="config-section" class="section loading">
          <div class="section-title">Settings</div>
          <div id="fields"></div>
          <div class="actions">
            <button id="save-btn" class="button">Save</button>
            <button id="reload-btn" class="button button-secondary">Reload</button>
          </div>
        </div>

        <div class="section">
          <div class="section-title">Agents</div>
          <div id="agent-section">
            <div class="agent-row">
              <input id="agent-goal" type="text" placeholder="Goal (e.g., Build the project)"/>
              <select id="agent-select">
                <option value="compiler">Compiler</option>
                <option value="deployer">Deployer</option>
                <option value="fixer">Fixer</option>
                <option value="tester">Tester</option>
              </select>
              <button id="run-agent-btn" class="button">Run</button>
            </div>
            <div id="agent-status-container"></div>
          </div>
        </div>

        <script>
          const vscode = acquireVsCodeApi();

          // DOM Elements
          const fieldsContainer = document.getElementById('fields');
          const saveBtn = document.getElementById('save-btn');
          const reloadBtn = document.getElementById('reload-btn');
          const errorContainer = document.getElementById('error-container');
          const configSection = document.getElementById('config-section');
          const agentSelect = document.getElementById('agent-select');
          const agentGoal = document.getElementById('agent-goal');
          const runAgentBtn = document.getElementById('run-agent-btn');
          const agentStatusContainer = document.getElementById('agent-status-container');

          let config = {};

          // Request config on load
          vscode.postMessage({ type: 'loadConfig' });

          saveBtn.addEventListener('click', () => {
            const values = {};
            const inputs = fieldsContainer.querySelectorAll('[data-key]');
            for (const input of inputs) {
              const key = input.getAttribute('data-key');
              values[key] = input.value;
            }
            vscode.postMessage({
              type: 'saveConfig',
              values: values,
            });
          });

          reloadBtn.addEventListener('click', () => {
            vscode.postMessage({ type: 'loadConfig' });
          });

          runAgentBtn.addEventListener('click', () => {
            const goal = agentGoal.value.trim();
            if (!goal) {
              showError('Please enter a goal');
              return;
            }
            const agent = agentSelect.value;
            vscode.postMessage({
              type: 'runAgent',
              agent: agent,
              goal: goal,
            });
          });

          agentGoal.addEventListener('keydown', (e) => {
            if (e.key === 'Enter') {
              runAgentBtn.click();
            }
          });

          function renderFields(config) {
            fieldsContainer.innerHTML = '';
            const fields = [
              { key: 'model', label: 'Default Model', type: 'select', options: ['gpt-4o', 'gpt-4o-mini', 'claude-3.5-sonnet', 'llama3'], default: 'gpt-4o' },
              { key: 'max_steps', label: 'Max Steps', type: 'number', default: 10, description: 'Maximum iterations per agent run' },
              { key: 'max_retries', label: 'Max Retries', type: 'number', default: 3, description: 'Number of retries on failure' },
              { key: 'temperature', label: 'Temperature', type: 'number', default: 0.7, description: 'Randomness (0.0 = deterministic, 1.0 = creative)' },
              { key: 'corePath', label: 'Core Binary Path', type: 'text', default: '', description: 'Path to the Spire core executable' },
            ];

            for (const field of fields) {
              const div = document.createElement('div');
              div.className = 'field';

              const label = document.createElement('label');
              label.textContent = field.label;
              div.appendChild(label);

              let input;
              if (field.type === 'select') {
                input = document.createElement('select');
                for (const option of field.options) {
                  const opt = document.createElement('option');
                  opt.value = option;
                  opt.textContent = option;
                  input.appendChild(opt);
                }
              } else {
                input = document.createElement('input');
                input.type = field.type;
              }

              const value = config[field.key] !== undefined ? config[field.key] : field.default;
              input.value = value;
              input.setAttribute('data-key', field.key);

              div.appendChild(input);

              if (field.description) {
                const desc = document.createElement('div');
                desc.className = 'description';
                desc.textContent = field.description;
                div.appendChild(desc);
              }

              fieldsContainer.appendChild(div);
            }

            configSection.classList.remove('loading');
          }

          function showError(message) {
            errorContainer.textContent = '❌ ' + message;
            errorContainer.classList.remove('hidden');
            setTimeout(() => {
              errorContainer.classList.add('hidden');
            }, 5000);
          }

          function updateAgentStatus(agent, status) {
            const div = document.createElement('div');
            div.className = 'agent-status';
            const statusText = status.status || 'unknown';
            const statusClass = statusText === 'running' ? 'running' : (statusText === 'failed' ? 'failed' : '');
            div.innerHTML = \`
              <strong>\${agent}</strong>: 
              <span class="\${statusClass}">\${statusText}</span>
              \${status.message ? ' - ' + status.message : ''}
              \${status.step ? ' (\${status.step}/\${status.total})' : ''}
            \`;
            agentStatusContainer.prepend(div);
            // Keep only last 5
            while (agentStatusContainer.children.length > 5) {
              agentStatusContainer.removeChild(agentStatusContainer.lastChild);
            }
          }

          // Handle messages from extension
          window.addEventListener('message', (event) => {
            const msg = event.data;

            switch (msg.type) {
              case 'configLoaded':
                config = msg.config;
                renderFields(config);
                break;

              case 'configChanged':
                renderFields(config);
                break;

              case 'agentResult':
                const status = {
                  status: 'running',
                  message: 'Started',
                  step: 1,
                  total: 1,
                };
                updateAgentStatus(msg.agent || 'unknown', status);
                break;

              case 'agentStatus':
                updateAgentStatus(msg.agent, msg.status);
                break;

              case 'error':
                showError(msg.error);
                break;
            }
          });
        </script>
      </body>
      </html>
    `;
  }
}
