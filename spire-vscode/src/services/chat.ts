import * as vscode from 'vscode';
import { McpClient } from '../mcp/client';

export interface ChatContext {
  filePath?: string;
  selection?: string;
  projectRoot?: string;
}

export interface ChatChunkEvent {
  sessionId: string;
  chunk: string;
  done: boolean;
}

export interface ProgressEvent {
  sessionId: string;
  step: number;
  total: number;
  message: string;
  status: 'running' | 'completed' | 'failed';
}

export interface CompleteEvent {
  sessionId: string;
  result: string;
  artifacts: string[];
  durationMs: number;
}

export interface ErrorEvent {
  sessionId: string;
  error: string;
  suggestion?: string;
}

export class ChatService {
  private client: McpClient;
  private sessionId: string;
  private onChunkHandlers: ((event: ChatChunkEvent) => void)[] = [];
  private onProgressHandlers: ((event: ProgressEvent) => void)[] = [];
  private onCompleteHandlers: ((event: CompleteEvent) => void)[] = [];
  private onErrorHandlers: ((event: ErrorEvent) => void)[] = [];

  constructor(client: McpClient) {
    this.client = client;
    this.sessionId = this.generateSessionId();

    // Register notification handlers
    this.client.onNotification('chat/chunk', (method, params) => {
      this.handleChunk(params);
    });

    this.client.onNotification('agent/progress', (method, params) => {
      this.handleProgress(params);
    });

    this.client.onNotification('agent/complete', (method, params) => {
      this.handleComplete(params);
    });

    this.client.onNotification('agent/error', (method, params) => {
      this.handleError(params);
    });
  }

  private generateSessionId(): string {
    return `sess_${Date.now()}_${Math.random().toString(36).substring(7)}`;
  }

  newSession(): void {
    this.sessionId = this.generateSessionId();
  }

  async sendMessage(message: string, context?: ChatContext): Promise<void> {
    await this.client.sendNotification('chat/send', {
      message,
      session_id: this.sessionId,
      context,
    });
  }

  async sendStreamingMessage(message: string, context?: ChatContext): Promise<void> {
    await this.client.callTool('chat/stream', {
      message,
      session_id: this.sessionId,
      context,
    });
  }

  private handleChunk(params: any): void {
    const event: ChatChunkEvent = {
      sessionId: params.session_id,
      chunk: params.chunk,
      done: params.done || false,
    };
    for (const handler of this.onChunkHandlers) {
      try {
        handler(event);
      } catch (e) {
        console.error('Error in chunk handler:', e);
      }
    }
  }

  private handleProgress(params: any): void {
    const event: ProgressEvent = {
      sessionId: params.session_id,
      step: params.step,
      total: params.total,
      message: params.message,
      status: params.status || 'running',
    };
    for (const handler of this.onProgressHandlers) {
      try {
        handler(event);
      } catch (e) {
        console.error('Error in progress handler:', e);
      }
    }
  }

  private handleComplete(params: any): void {
    const event: CompleteEvent = {
      sessionId: params.session_id,
      result: params.result,
      artifacts: params.artifacts || [],
      durationMs: params.duration_ms || 0,
    };
    for (const handler of this.onCompleteHandlers) {
      try {
        handler(event);
      } catch (e) {
        console.error('Error in complete handler:', e);
      }
    }
  }

  private handleError(params: any): void {
    const event: ErrorEvent = {
      sessionId: params.session_id,
      error: params.error,
      suggestion: params.suggestion,
    };
    for (const handler of this.onErrorHandlers) {
      try {
        handler(event);
      } catch (e) {
        console.error('Error in error handler:', e);
      }
    }
  }

  onChunk(handler: (event: ChatChunkEvent) => void): vscode.Disposable {
    this.onChunkHandlers.push(handler);
    return { dispose: () => { this.onChunkHandlers = this.onChunkHandlers.filter(h => h !== handler); } };
  }

  onProgress(handler: (event: ProgressEvent) => void): vscode.Disposable {
    this.onProgressHandlers.push(handler);
    return { dispose: () => { this.onProgressHandlers = this.onProgressHandlers.filter(h => h !== handler); } };
  }

  onComplete(handler: (event: CompleteEvent) => void): vscode.Disposable {
    this.onCompleteHandlers.push(handler);
    return { dispose: () => { this.onCompleteHandlers = this.onCompleteHandlers.filter(h => h !== handler); } };
  }

  onError(handler: (event: ErrorEvent) => void): vscode.Disposable {
    this.onErrorHandlers.push(handler);
    return { dispose: () => { this.onErrorHandlers = this.onErrorHandlers.filter(h => h !== handler); } };
  }

  getSessionId(): string {
    return this.sessionId;
  }
}
