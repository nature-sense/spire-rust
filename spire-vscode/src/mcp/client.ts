import * as child_process from 'child_process';
import * as readline from 'readline';
import * as fs from 'fs';
import * as path from 'path';
import { McpRequest, McpResponse, McpNotification, McpMessage, ToolCallParams } from './types';
import * as vscode from 'vscode';

export type NotificationHandler = (method: string, params: any) => void;

export class McpClient {
  private process: child_process.ChildProcess | null = null;
  private rl: readline.Interface | null = null;
  private requestId: number = 0;
  private pendingRequests: Map<number, { resolve: (value: any) => void; reject: (reason: any) => void }> = new Map();
  private notificationHandlers: Map<string, NotificationHandler[]> = new Map();
  private connected: boolean = false;
  private reconnectAttempts: number = 0;
  private maxReconnectAttempts: number = 3;

  constructor() {}

  async start(serverPath: string): Promise<void> {
    // Resolve path
    const resolvedPath = this.resolvePath(serverPath);

    if (!fs.existsSync(resolvedPath)) {
      throw new Error(`Spire core not found at: ${resolvedPath}`);
    }

    // Make executable
    try {
      fs.chmodSync(resolvedPath, 0o755);
    } catch (e) {
      // Ignore on Windows
    }

    console.log(`Starting Spire core: ${resolvedPath}`);

    this.process = child_process.spawn(resolvedPath, [], {
      stdio: ['pipe', 'pipe', 'pipe'],
      env: process.env,
    });

    this.process.on('error', (err) => {
      console.error('MCP process error:', err);
      this.handleDisconnect();
    });

    this.process.on('exit', (code) => {
      console.log(`MCP process exited with code ${code}`);
      this.handleDisconnect();
    });

    // Set up stdin/stdout
    this.rl = readline.createInterface({
      input: this.process.stdout!,
      crlfDelay: Infinity,
    });

    this.rl.on('line', (line: string) => {
      try {
        const msg: McpMessage = JSON.parse(line);
        this.handleMessage(msg);
      } catch (e) {
        console.error('Failed to parse MCP message:', e);
      }
    });

    // Log stderr
    this.process.stderr?.on('data', (data) => {
      console.error('MCP stderr:', data.toString());
      vscode.window.showErrorMessage(`Spire core: ${data.toString()}`);
    });

    this.connected = true;
    this.reconnectAttempts = 0;
    console.log('Spire core connected');
  }

  private resolvePath(serverPath: string): string {
    if (path.isAbsolute(serverPath)) {
      return serverPath;
    }

    // Check in extension directory
    const extensionPath = vscode.extensions.getExtension('spire.spire-vscode')?.extensionPath;
    if (extensionPath) {
      const fullPath = path.join(extensionPath, serverPath);
      if (fs.existsSync(fullPath)) {
        return fullPath;
      }
    }

    // Check in PATH
    try {
      const which = require('which');
      const found = which.sync(serverPath);
      if (found) {
        return found;
      }
    } catch (e) {
      // Not in PATH
    }

    return serverPath;
  }

  private handleMessage(msg: McpMessage): void {
    // Handle responses
    if ('id' in msg) {
      const pending = this.pendingRequests.get(msg.id);
      if (pending) {
        this.pendingRequests.delete(msg.id);
        if (msg.error) {
          pending.reject(new Error(msg.error.message));
        } else {
          pending.resolve(msg.result);
        }
      }
    }

    // Handle notifications
    if ('method' in msg) {
      this.handleNotification(msg.method, msg.params);
    }
  }

  private handleNotification(method: string, params: any): void {
    const handlers = this.notificationHandlers.get(method) || [];
    for (const handler of handlers) {
      try {
        handler(method, params);
      } catch (e) {
        console.error('Error in notification handler:', e);
      }
    }
  }

  private handleDisconnect(): void {
    this.connected = false;
    this.rl = null;

    // Reject all pending requests
    for (const [, pending] of this.pendingRequests) {
      pending.reject(new Error('MCP connection lost'));
    }
    this.pendingRequests.clear();

    // Try to reconnect
    if (this.reconnectAttempts < this.maxReconnectAttempts) {
      this.reconnectAttempts++;
      const delay = Math.min(1000 * Math.pow(2, this.reconnectAttempts - 1), 10000);
      console.log(`Reconnecting in ${delay}ms...`);
      setTimeout(() => {
        const config = vscode.workspace.getConfiguration('spire');
        const corePath = config.get<string>('corePath', '');
        if (corePath) {
          this.start(corePath).catch(console.error);
        }
      }, delay);
    }
  }

  async callTool(tool: string, args: Record<string, any>): Promise<any> {
    if (!this.connected || !this.process || !this.process.stdin) {
      throw new Error('MCP client not connected');
    }

    const id = ++this.requestId;
    const params: ToolCallParams = { name: tool, arguments: args };

    const request: McpRequest = {
      jsonrpc: '2.0',
      id,
      method: 'tools/call',
      params,
    };

    return new Promise((resolve, reject) => {
      this.pendingRequests.set(id, { resolve, reject });

      // Timeout after 60 seconds for normal calls, 300 seconds for streaming
      const timeout = tool === 'chat/stream' ? 300000 : 60000;
      const timeoutId = setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error('MCP request timed out'));
        }
      }, timeout);

      try {
        this.process!.stdin!.write(JSON.stringify(request) + '\n');
      } catch (e) {
        clearTimeout(timeoutId);
        this.pendingRequests.delete(id);
        reject(e);
      }
    });
  }

  async sendNotification(method: string, params: any): Promise<void> {
    if (!this.connected || !this.process || !this.process.stdin) {
      throw new Error('MCP client not connected');
    }

    const notification: McpNotification = {
      jsonrpc: '2.0',
      method,
      params,
    };

    this.process.stdin.write(JSON.stringify(notification) + '\n');
  }

  onNotification(method: string, handler: NotificationHandler): vscode.Disposable {
    if (!this.notificationHandlers.has(method)) {
      this.notificationHandlers.set(method, []);
    }
    this.notificationHandlers.get(method)!.push(handler);

    return {
      dispose: () => {
        const handlers = this.notificationHandlers.get(method);
        if (handlers) {
          const index = handlers.indexOf(handler);
          if (index !== -1) {
            handlers.splice(index, 1);
          }
        }
      }
    };
  }

  dispose(): any {
    return this.close();
  }

  async close(): Promise<void> {
    if (this.process) {
      this.process.stdin?.end();
      this.process.kill();
      this.process = null;
    }
    this.connected = false;
    this.rl = null;
  }

  isConnected(): boolean {
    return this.connected;
  }
}
