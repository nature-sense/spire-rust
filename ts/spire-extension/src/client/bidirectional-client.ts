import { spawn, ChildProcess } from 'child_process';
import * as net from 'net';
import { createInterface, Interface as ReadlineInterface } from 'readline';
import { logger } from '../util/logger';
import type { Router } from '../server/router';

/**
 * Pending outgoing request tracked by the client.
 */
interface PendingRequest {
  resolve: (value: unknown) => void;
  reject: (reason: Error) => void;
  timer: ReturnType<typeof setTimeout>;
  method: string;
}

/**
 * Handler for server-pushed notifications (JSON-RPC without id).
 */
type NotificationHandler = (method: string, params: unknown) => void;

/**
 * Options for the BidirectionalClient.
 */
export interface BidirectionalClientOptions {
  /** Path to the subprocess binary/script. */
  command: string;
  /** Arguments to pass to the command. */
  args?: string[];
  /** Environment variables. */
  env?: Record<string, string>;
  /** Request timeout in milliseconds (default: 30000). */
  timeout?: number;
  /** Whether to automatically restart on exit (default: false). */
  autoRestart?: boolean;
}

/**
 * Bidirectional JSON-RPC 2.0 client over TCP socket.
 *
 * This client:
 * 1. Spawns the subprocess, reads "SPIRE_PORT=<port>" from stdout
 * 2. Connects to the subprocess via TCP on 127.0.0.1:<port>
 * 3. Sends outgoing requests to the subprocess and matches responses
 * 4. Handles incoming requests FROM the subprocess by routing them
 *    to a local Router (which has VS Code API handlers registered)
 * 5. Handles notifications (no id) from the subprocess
 *
 * Architecture:
 *   Extension Host (this process)          Subprocess (Rust core)
 *   ┌─────────────────────────────┐       ┌──────────────────────┐
 *   │ BidirectionalClient         │  TCP  │ Transport (socket)   │
 *   │   ├── outgoing reqs ────────┼──────►│   ├── agents         │
 *   │   ├── incoming reqs ◄───────┼───────┤   ├── chat           │
 *   │   └── local Router         │       │   └── tools          │
 *   │       ├── workspace.ts     │       └──────────────────────┘
 *   │       ├── editor.ts        │
 *   │       ├── ...              │
 *   └─────────────────────────────┘
 */
export class BidirectionalClient {
  private proc: ChildProcess | null = null;
  private socket: net.Socket | null = null;
  private rl: ReadlineInterface | null = null;
  private pending = new Map<number, PendingRequest>();
  private nextId = 1;
  private notificationHandlers = new Map<string, NotificationHandler[]>();
  private options: Required<BidirectionalClientOptions>;
  private _isRunning = false;
  private buffer = '';
  private localRouter: Router | null = null;

  constructor(options: BidirectionalClientOptions) {
    this.options = {
      command: options.command,
      args: options.args ?? [],
      env: options.env ?? { ...process.env } as Record<string, string>,
      timeout: options.timeout ?? 30000,
      autoRestart: options.autoRestart ?? false,
    };
  }

  /** Whether the client is currently connected to a running subprocess. */
  get isRunning(): boolean {
    return this._isRunning;
  }

  /**
   * Register the local router that handles incoming requests from the subprocess.
   * The router should have VS Code API handlers registered (workspace, editor, etc.).
   */
  setLocalRouter(router: Router): void {
    this.localRouter = router;
  }

  /**
   * Start the subprocess and connect to it via TCP.
   */
  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this._isRunning) {
        reject(new Error('Client is already running'));
        return;
      }

      logger.info(`Starting subprocess: ${this.options.command} ${this.options.args.join(' ')}`);

      this.proc = spawn(this.options.command, this.options.args, {
        // stdin: ignore (we don't write to stdin anymore)
        // stdout: pipe (we read the port from the first line)
        // stderr: pipe (for diagnostics)
        stdio: ['ignore', 'pipe', 'pipe'],
        env: this.options.env,
      });

      this.proc.on('error', (err) => {
        logger.error(`Subprocess error: ${err.message}`);
        this._isRunning = false;
        this.rejectAllPending(err);
        reject(err);
      });

      this.proc.on('exit', (code, signal) => {
        logger.info(`Subprocess exited (code=${code}, signal=${signal})`);
        this._isRunning = false;
        this.cleanupSocket();
        const err = new Error(
          `Subprocess exited unexpectedly (code=${code}, signal=${signal})`
        );
        this.rejectAllPending(err);

        if (this.options.autoRestart) {
          logger.info('Auto-restarting subprocess...');
          this.start().catch((e) => logger.error(`Auto-restart failed: ${e.message}`));
        }
      });

      // Collect stderr for diagnostics
      this.proc.stderr?.on('data', (chunk: Buffer) => {
        const lines = chunk.toString().trim();
        if (lines) {
          logger.warn(`[subprocess stderr] ${lines}`);
        }
      });

      // Read the first line from stdout to discover the port
      let portDiscovered = false;
      const stdoutBuffer: string[] = [];

      this.proc.stdout!.on('data', (chunk: Buffer) => {
        const text = chunk.toString();
        stdoutBuffer.push(text);

        // Check if we have a complete line with the port
        const fullText = stdoutBuffer.join('');
        const match = fullText.match(/^SPIRE_PORT=(\d+)\s*\n/);
        if (match && !portDiscovered) {
          portDiscovered = true;
          const port = parseInt(match[1], 10);
          logger.info(`Discovered subprocess port: ${port}`);

          // Connect to the subprocess via TCP
          this.connectToSocket(port, resolve, reject);
        }
      });

      // Timeout for port discovery
      // Note: The core now prints SPIRE_PORT immediately after binding the TCP
      // listener, before the blocking initialization (embedding model download).
      // 30s is a generous safety net for edge cases.
      const portTimeout = setTimeout(() => {
        if (!portDiscovered) {
          this._isRunning = false;
          const err = new Error('Timed out waiting for SPIRE_PORT from subprocess');
          this.rejectAllPending(err);
          reject(err);
        }
      }, 30000);

    });
  }

  /**
   * Connect to the subprocess via TCP socket.
   */
  private connectToSocket(port: number, resolve: () => void, reject: (err: Error) => void): void {
    this.socket = net.createConnection({ host: '127.0.0.1', port }, () => {
      logger.info(`Connected to subprocess on 127.0.0.1:${port}`);

      // Set up readline on the socket for JSON-RPC messages
      this.rl = createInterface({
        input: this.socket!,
        crlfDelay: Infinity,
      });

      this.rl.on('line', (line: string) => {
        this.handleLine(line);
      });

      this.rl.on('close', () => {
        logger.info('Socket readline closed');
      });

      this._isRunning = true;
      resolve();
    });

    this.socket.on('error', (err) => {
      logger.error(`Socket error: ${err.message}`);
      this._isRunning = false;
      this.rejectAllPending(err);
      reject(err);
    });

    this.socket.on('close', () => {
      logger.info('Socket closed');
      this._isRunning = false;
      this.rl?.close();
      this.rl = null;
    });
  }

  /**
   * Stop the subprocess gracefully (SIGTERM), then SIGKILL after timeout.
   */
  async stop(timeoutMs = 5000): Promise<void> {
    if (!this.proc || !this._isRunning) {
      return;
    }

    logger.info('Stopping subprocess...');

    return new Promise((resolve) => {
      const killTimer = setTimeout(() => {
        if (this.proc && !this.proc.killed) {
          logger.warn('Force killing subprocess');
          this.proc.kill('SIGKILL');
        }
      }, timeoutMs);

      this.proc!.on('exit', () => {
        clearTimeout(killTimer);
        this._isRunning = false;
        this.cleanupSocket();
        this.proc = null;
        resolve();
      });

      this.proc!.kill('SIGTERM');
    });
  }

  /**
   * Restart the subprocess.
   */
  async restart(): Promise<void> {
    await this.stop();
    await this.start();
  }

  /**
   * Send a JSON-RPC request to the subprocess and wait for the response.
   *
   * @param method - The method name (e.g. "chat/getActive")
   * @param params - The parameters object
   * @returns A promise that resolves with the result
   */
  call<T = unknown>(method: string, params: unknown = {}): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (!this._isRunning || !this.socket) {
        reject(new Error('Client is not running'));
        return;
      }

      const id = this.nextId++;
      const request = JSON.stringify({
        jsonrpc: '2.0',
        id,
        method,
        params,
      });

      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Request timed out: ${method} (id=${id})`));
      }, this.options.timeout);

      this.pending.set(id, {
        resolve: resolve as (value: unknown) => void,
        reject,
        timer,
        method,
      });

      this.socket.write(request + '\n');
    });
  }

  /**
   * Send a JSON-RPC notification (no response expected) to the subprocess.
   */
  notify(method: string, params: unknown = {}): void {
    if (!this._isRunning || !this.socket) {
      logger.error('Cannot send notification: client is not running');
      return;
    }

    const notification = JSON.stringify({
      jsonrpc: '2.0',
      method,
      params,
    });

    this.socket.write(notification + '\n');
  }

  /**
   * Register a handler for server-pushed notifications.
   */
  onNotification(method: string, handler: NotificationHandler): void {
    const handlers = this.notificationHandlers.get(method) ?? [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);
  }

  /**
   * Remove a notification handler.
   */
  offNotification(method: string, handler: NotificationHandler): void {
    const handlers = this.notificationHandlers.get(method);
    if (handlers) {
      const idx = handlers.indexOf(handler);
      if (idx >= 0) {
        handlers.splice(idx, 1);
      }
    }
  }

  /**
   * Register a catch-all handler for all notifications.
   */
  onAnyNotification(handler: NotificationHandler): void {
    this.onNotification('*', handler);
  }

  // ── Private ──────────────────────────────────────────────────────────────

  private handleLine(line: string): void {
    // Handle partial/buffered JSON (in case of split packets)
    this.buffer += line;

    let parsed: unknown;
    try {
      parsed = JSON.parse(this.buffer);
      this.buffer = '';
    } catch {
      // Incomplete JSON, wait for more data
      return;
    }

    const msg = parsed as Record<string, unknown>;

    // Check if it's a notification (no id field)
    if (msg.id === undefined || msg.id === null) {
      this.handleNotification(msg);
      return;
    }

    const id = Number(msg.id);

    // Check if this is an incoming request from the subprocess
    // (an id we didn't generate — the subprocess uses its own ID namespace)
    if (!this.pending.has(id)) {
      // This is an incoming request from the subprocess
      this.handleIncomingRequest(id, msg);
      return;
    }

    // It's a response to one of our outgoing requests
    const pending = this.pending.get(id);
    if (!pending) {
      logger.warn(`Received response for unknown request id=${id}`);
      return;
    }

    clearTimeout(pending.timer);
    this.pending.delete(id);

    if (msg.error) {
      const err = msg.error as { code?: number; message?: string; data?: unknown };
      pending.reject(new Error(`JSON-RPC error (${err.code ?? -32603}): ${err.message ?? 'Unknown error'}`));
    } else {
      pending.resolve(msg.result);
    }
  }

  /**
   * Handle an incoming request from the subprocess.
   * Routes it to the local router (which has VS Code API handlers) and sends a response back.
   */
  private async handleIncomingRequest(id: number, msg: Record<string, unknown>): Promise<void> {
    const method = msg.method as string;
    const params = msg.params;

    logger.info(`Received incoming request from subprocess: ${method} (id=${id})`);

    if (!this.localRouter) {
      logger.error(`No local router registered — cannot handle incoming request: ${method} (id=${id})`);
      this.sendToSocket({
        jsonrpc: '2.0',
        id,
        error: { code: -32601, message: `No local handler registered for: ${method}` },
      });
      return;
    }

    try {
      const request = {
        jsonrpc: '2.0' as const,
        id,
        method,
        params,
      };
      logger.debug(`Routing incoming request to local router: ${method} (id=${id})`);
      const result = await this.localRouter.handle(request);
      logger.debug(`Sending response for incoming request: ${method} (id=${id})`);
      this.sendToSocket({
        jsonrpc: '2.0',
        id,
        result,
      });
    } catch (err: unknown) {
      const errorObj = err as { code?: number; message?: string; data?: unknown };
      logger.error(`Error handling incoming request ${method} (id=${id}): ${errorObj.message ?? 'Unknown error'}`);
      this.sendToSocket({
        jsonrpc: '2.0',
        id,
        error: {
          code: errorObj.code ?? -32603,
          message: errorObj.message ?? 'Internal error',
          data: errorObj.data,
        },
      });
    }
  }

  private handleNotification(msg: Record<string, unknown>): void {
    const method = msg.method as string;
    const params = msg.params;

    logger.info(`Received notification: ${method}`);

    // Call specific method handlers
    const handlers = this.notificationHandlers.get(method);
    if (handlers) {
      for (const handler of handlers) {
        try {
          handler(method, params);
        } catch (err) {
          logger.error(`Error in notification handler for ${method}: ${err}`);
        }
      }
    }

    // Call catch-all handlers
    const catchAll = this.notificationHandlers.get('*');
    if (catchAll) {
      for (const handler of catchAll) {
        try {
          handler(method, params);
        } catch (err) {
          logger.error(`Error in catch-all notification handler: ${err}`);
        }
      }
    }
  }

  private sendToSocket(message: Record<string, unknown>): void {
    if (!this.socket) {
      logger.error('Cannot send to subprocess: socket not connected');
      return;
    }
    const json = JSON.stringify(message);
    this.socket.write(json + '\n');
  }

  private cleanupSocket(): void {
    this.rl?.close();
    this.rl = null;
    if (this.socket) {
      this.socket.destroy();
      this.socket = null;
    }
  }

  private rejectAllPending(err: Error): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }
}
