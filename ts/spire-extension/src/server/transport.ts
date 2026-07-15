import * as readline from 'node:readline';
import { logger } from '../util/logger';
import type { JsonRpcRequest, JsonRpcResponse, JsonRpcNotification } from '../model/messages';

/**
 * Callback type for handling an incoming JSON-RPC request.
 * Returns a result value or throws a JsonRpcError-compatible object.
 */
export type RequestHandler = (
  request: JsonRpcRequest
) => Promise<unknown>;

/**
 * Manages stdin/stdout JSON-RPC 2.0 transport.
 *
 * - Reads newline-delimited JSON from stdin
 * - Writes newline-delimited JSON responses to stdout
 * - Supports sending notifications (events) to the client via stdout
 */
export class Transport {
  private rl: readline.Interface;
  private handler: RequestHandler | null = null;
  private requestIdCounter = 0;

  constructor() {
    this.rl = readline.createInterface({
      input: process.stdin,
      crlfDelay: Infinity,
    });
  }

  /** Set the handler for incoming requests */
  onRequest(handler: RequestHandler): void {
    this.handler = handler;
  }

  /** Start listening on stdin */
  start(): void {
    this.rl.on('line', (line: string) => {
      const trimmed = line.trim();
      if (!trimmed) return;
      this.handleLine(trimmed).catch((err) => {
        logger.error('Unhandled transport error:', err);
      });
    });

    this.rl.on('close', () => {
      logger.info('stdin closed, shutting down');
      process.exit(0);
    });

    logger.info('JSON-RPC transport started (stdin/stdout)');
  }

  /** Send a notification (event) to the client */
  sendNotification(method: string, params: unknown): void {
    const notification: JsonRpcNotification = {
      jsonrpc: '2.0',
      method,
      params,
    };
    this.writeMessage(notification);
  }

  /** Send a response to a specific request */
  sendResponse(id: number, result: unknown): void {
    const response: JsonRpcResponse = {
      jsonrpc: '2.0',
      id,
      result,
    };
    this.writeMessage(response);
  }

  /** Send an error response */
  sendError(id: number, code: number, message: string, data?: unknown): void {
    const response: JsonRpcResponse = {
      jsonrpc: '2.0',
      id,
      error: { code, message, data },
    };
    this.writeMessage(response);
  }

  /** Create a new request ID (for client-side use) */
  nextId(): number {
    return ++this.requestIdCounter;
  }

  private async handleLine(line: string): Promise<void> {
    let request: JsonRpcRequest;
    try {
      request = JSON.parse(line) as JsonRpcRequest;
    } catch {
      // Parse error
      this.sendError(-1, -32700, 'Parse error');
      return;
    }

    // Validate JSON-RPC 2.0
    if (request.jsonrpc !== '2.0' || !request.method) {
      this.sendError(
        request.id ?? -1,
        -32600,
        'Invalid request: must have jsonrpc:"2.0" and a method'
      );
      return;
    }

    // Notification (no id) — fire-and-forget, no response
    if (request.id === undefined || request.id === null) {
      logger.debug('Received notification:', request.method);
      if (this.handler) {
        this.handler(request).catch((err) => {
          logger.error('Handler error for notification:', err);
        });
      }
      return;
    }

    // Request (has id)
    logger.debug('Received request:', request.method, `(id=${request.id})`);

    if (!this.handler) {
      this.sendError(request.id, -32601, 'Method not found: no handler registered');
      return;
    }

    try {
      const result = await this.handler(request);
      this.sendResponse(request.id, result);
    } catch (err: unknown) {
      const errorObj = err as { code?: number; message?: string; data?: unknown };
      const code = errorObj.code ?? -32603;
      const message = errorObj.message ?? 'Internal error';
      const data = errorObj.data;
      this.sendError(request.id, code, message, data);
    }
  }

  private writeMessage(msg: JsonRpcResponse | JsonRpcNotification): void {
    const json = JSON.stringify(msg);
    process.stdout.write(json + '\n');
  }
}
