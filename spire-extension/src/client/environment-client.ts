import { spawn, ChildProcess } from 'child_process';
import { createInterface, Interface as ReadlineInterface } from 'readline';
import { logger } from '../util/logger';

/**
 * Pending request tracked by the client.
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
 * Options for the EnvironmentClient.
 */
export interface EnvironmentClientOptions {
  /** Path to the environment server binary/script. */
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
 * Client that communicates with an environment server subprocess
 * via JSON-RPC over stdin/stdout.
 *
 * Usage:
 * ```ts
 * const client = new EnvironmentClient({
 *   command: 'node',
 *   args: ['dist/env-server.js'],
 * });
 * await client.start();
 * const result = await client.call('chat/getActive', {});
 * await client.stop();
 * ```
 */
export class EnvironmentClient {
  private proc: ChildProcess | null = null;
  private rl: ReadlineInterface | null = null;
  private pending = new Map<number, PendingRequest>();
  private nextId = 1;
  private notificationHandlers = new Map<string, NotificationHandler[]>();
  private options: Required<EnvironmentClientOptions>;
  private _isRunning = false;
  private buffer = '';

  constructor(options: EnvironmentClientOptions) {
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
   * Start the subprocess and begin reading responses.
   */
  start(): Promise<void> {
    return new Promise((resolve, reject) => {
      if (this._isRunning) {
        reject(new Error('Client is already running'));
        return;
      }

      logger.info(`Starting environment server: ${this.options.command} ${this.options.args.join(' ')}`);

      this.proc = spawn(this.options.command, this.options.args, {
        stdio: ['pipe', 'pipe', 'pipe'],
        env: this.options.env,
      });

      this.proc.on('error', (err) => {
        logger.error(`Environment server process error: ${err.message}`);
        this._isRunning = false;
        this.rejectAllPending(err);
        reject(err);
      });

      this.proc.on('exit', (code, signal) => {
        logger.info(`Environment server exited (code=${code}, signal=${signal})`);
        this._isRunning = false;
        const err = new Error(
          `Environment server exited unexpectedly (code=${code}, signal=${signal})`
        );
        this.rejectAllPending(err);

        if (this.options.autoRestart) {
          logger.info('Auto-restarting environment server...');
          this.start().catch((e) => logger.error(`Auto-restart failed: ${e.message}`));
        }
      });

      // Collect stderr for diagnostics
      this.proc.stderr?.on('data', (chunk: Buffer) => {
        const lines = chunk.toString().trim();
        if (lines) {
          logger.warn(`[env-server stderr] ${lines}`);
        }
      });

      // Set up readline on stdout for JSON-RPC responses
      this.rl = createInterface({
        input: this.proc.stdout!,
        crlfDelay: Infinity,
      });

      this.rl.on('line', (line: string) => {
        this.handleLine(line);
      });

      this.rl.on('close', () => {
        // stdout stream ended
      });

      // Give the process a moment to start, then resolve
      // We use a small delay to let the process initialize
      setImmediate(() => {
        this._isRunning = true;
        resolve();
      });
    });
  }

  /**
   * Stop the subprocess gracefully (SIGTERM), then SIGKILL after timeout.
   */
  async stop(timeoutMs = 5000): Promise<void> {
    if (!this.proc || !this._isRunning) {
      return;
    }

    logger.info('Stopping environment server...');

    return new Promise((resolve) => {
      const killTimer = setTimeout(() => {
        if (this.proc && !this.proc.killed) {
          logger.warn('Force killing environment server');
          this.proc.kill('SIGKILL');
        }
      }, timeoutMs);

      this.proc!.on('exit', () => {
        clearTimeout(killTimer);
        this._isRunning = false;
        this.rl?.close();
        this.rl = null;
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
   * Send a JSON-RPC request and wait for the response.
   *
   * @param method - The method name (e.g. "chat/getActive")
   * @param params - The parameters object
   * @returns A promise that resolves with the result
   */
  call<T = unknown>(method: string, params: unknown = {}): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      if (!this._isRunning || !this.proc) {
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

      this.proc.stdin!.write(request + '\n');
    });
  }

  /**
   * Send a JSON-RPC notification (no response expected).
   */
  notify(method: string, params: unknown = {}): void {
    if (!this._isRunning || !this.proc) {
      logger.error('Cannot send notification: client is not running');
      return;
    }

    const notification = JSON.stringify({
      jsonrpc: '2.0',
      method,
      params,
    });

    this.proc.stdin!.write(notification + '\n');
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

    // It's a response — match to pending request
    const id = Number(msg.id);
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

  private rejectAllPending(err: Error): void {
    for (const [id, pending] of this.pending) {
      clearTimeout(pending.timer);
      pending.reject(err);
    }
    this.pending.clear();
  }
}
