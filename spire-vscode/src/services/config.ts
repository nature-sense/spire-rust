import * as vscode from 'vscode';
import { McpClient } from '../mcp/client';

export interface ConfigChangeEvent {
  key: string;
  oldValue: any;
  newValue: any;
}

export class ConfigService {
  private client: McpClient;
  private onConfigChangedHandlers: ((event: ConfigChangeEvent) => void)[] = [];

  constructor(client: McpClient) {
    this.client = client;

    this.client.onNotification('config/changed', (method, params) => {
      this.handleConfigChanged(params);
    });
  }

  async getConfig(keys?: string[]): Promise<Record<string, any>> {
    return this.client.callTool('config/get', { keys });
  }

  async setConfig(values: Record<string, any>): Promise<void> {
    await this.client.callTool('config/set', { values });
  }

  async getAgentStatus(agentId: string): Promise<any> {
    return this.client.callTool('agent/status', { agent_id: agentId });
  }

  async runAgent(agent: string, goal: string, project?: string): Promise<any> {
    return this.client.callTool('agent/run', {
      agent,
      goal,
      project,
    });
  }

  private handleConfigChanged(params: any): void {
    const event: ConfigChangeEvent = {
      key: params.key,
      oldValue: params.old_value,
      newValue: params.new_value,
    };
    for (const handler of this.onConfigChangedHandlers) {
      try {
        handler(event);
      } catch (e) {
        console.error('Error in config changed handler:', e);
      }
    }
  }

  onConfigChanged(handler: (event: ConfigChangeEvent) => void): vscode.Disposable {
    this.onConfigChangedHandlers.push(handler);
    return { dispose: () => { this.onConfigChangedHandlers = this.onConfigChangedHandlers.filter(h => h !== handler); } };
  }
}
