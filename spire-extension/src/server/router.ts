import { logger } from '../util/logger';
import type { JsonRpcRequest } from '../model/messages';
import { ErrorCode } from '../model/messages';
import type { MethodName } from '../model/messages';

/**
 * Handler function for a specific JSON-RPC method.
 * Receives parsed params and returns a result.
 */
export type MethodHandler = (params: unknown) => Promise<unknown>;

/**
 * Routes incoming JSON-RPC requests to registered method handlers.
 */
export class Router {
  private handlers = new Map<string, MethodHandler>();

  /** Register a handler for a method */
  on(method: string, handler: MethodHandler): void {
    if (this.handlers.has(method)) {
      logger.warn(`Router: overwriting handler for method '${method}'`);
    }
    this.handlers.set(method, handler);
    logger.debug(`Router: registered handler for '${method}'`);
  }

  /** Register multiple handlers at once */
  registerAll(handlers: Record<string, MethodHandler>): void {
    for (const [method, handler] of Object.entries(handlers)) {
      this.on(method, handler);
    }
  }

  /** Handle an incoming request — called by Transport */
  async handle(request: JsonRpcRequest): Promise<unknown> {
    const method = request.method as MethodName;
    const handler = this.handlers.get(method);

    if (!handler) {
      const err = new Error(`Method not found: ${method}`);
      (err as unknown as Record<string, unknown>).code = ErrorCode.MethodNotFound;
      throw err;
    }

    logger.debug(`Router: dispatching '${method}'`);
    return handler(request.params);
  }
}
