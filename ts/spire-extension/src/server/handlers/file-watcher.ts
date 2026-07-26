import * as vscode from 'vscode';
import { logger } from '../../util/logger';

/**
 * File Watcher — monitors file system changes and forwards them to the
 * Rust core as FileEvent notifications for continuous project sync.
 *
 * This is Phase 3 of the three-phase project sync:
 *   1. Bootstrap (cold start) — full scan + create-all
 *   2. Startup sync (warm start) — content-hash manifest diff
 *   3. Continuous sync (real-time) — file change events from this watcher
 *
 * The watcher uses VS Code's `createFileSystemWatcher` with a debounce
 * to batch rapid changes (e.g., git checkout, npm install).
 */

/**
 * Debounce helper: returns a function that delays invoking `fn` until
 * `delayMs` milliseconds have elapsed since the last invocation.
 */
function debounce<T extends (...args: any[]) => void>(
  fn: T,
  delayMs: number,
): (...args: Parameters<T>) => void {
  let timer: ReturnType<typeof setTimeout> | undefined;
  return (...args: Parameters<T>) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = undefined;
      fn(...args);
    }, delayMs);
  };
}

/**
 * File change event types matching the Rust `ChangeType` enum.
 */
type ChangeType = 'Created' | 'Modified' | 'Deleted';

/**
 * A queued file event awaiting dispatch.
 */
interface QueuedEvent {
  changeType: ChangeType;
  path: string;
}

/**
 * FileWatcher class — manages the lifecycle of the VS Code file system watcher
 * and forwards events to the core via a notification callback.
 */
export class FileWatcher {
  private watcher: vscode.FileSystemWatcher | null = null;
  private disposables: vscode.Disposable[] = [];
  private eventQueue: QueuedEvent[] = [];
  private flushTimer: ReturnType<typeof setTimeout> | null = null;

  /** Callback to forward events to the core subprocess. */
  private notifyCore: ((method: string, params: unknown) => void) | null = null;

  /** Debounce window in milliseconds (matches Rust's FILE_EVENT_DEBOUNCE_MS). */
  private readonly debounceMs = 500;

  /**
   * Create a file watcher for the given workspace folder.
   *
   * @param workspacePath - Absolute path to the workspace root.
   * @param notifyCore - Callback to send notifications to the core subprocess.
   */
  constructor(
    private readonly workspacePath: string,
    notifyCore: (method: string, params: unknown) => void,
  ) {
    this.notifyCore = notifyCore;
  }

  /**
   * Start watching the workspace for file changes.
   *
   * Uses a single glob pattern `**` to watch all files recursively.
   * VS Code's watcher is efficient — it uses OS-level file system events
   * under the hood and does not poll.
   */
  start(): void {
    if (this.watcher) {
      logger.warn('FileWatcher: already started, ignoring duplicate start');
      return;
    }

    logger.info(`FileWatcher: starting for workspace: ${this.workspacePath}`);

    // Create the watcher with a broad pattern.
    // `**` watches all files recursively. We exclude common non-project dirs
    // via the Rust-side filter (SKIP_DIRS in project_sync.rs).
    const pattern = new vscode.RelativePattern(this.workspacePath, '**');
    this.watcher = vscode.workspace.createFileSystemWatcher(pattern);

    // Register event handlers with debounced dispatch
    this.disposables.push(
      this.watcher.onDidCreate((uri) => this.queueEvent('Created', uri)),
      this.watcher.onDidChange((uri) => this.queueEvent('Modified', uri)),
      this.watcher.onDidDelete((uri) => this.queueEvent('Deleted', uri)),
    );

    logger.info('FileWatcher: started successfully');
  }

  /**
   * Stop watching and clean up all disposables.
   */
  stop(): void {
    logger.info('FileWatcher: stopping');

    if (this.flushTimer) {
      clearTimeout(this.flushTimer);
      this.flushTimer = null;
    }

    for (const disposable of this.disposables) {
      disposable.dispose();
    }
    this.disposables = [];

    if (this.watcher) {
      this.watcher.dispose();
      this.watcher = null;
    }

    this.eventQueue = [];
    logger.info('FileWatcher: stopped');
  }

  /**
   * Queue a file event for debounced dispatch.
   * Multiple rapid events for the same path are coalesced.
   */
  private queueEvent(changeType: ChangeType, uri: vscode.Uri): void {
    const path = uri.fsPath;

    // Skip files outside the workspace (shouldn't happen with RelativePattern,
    // but be safe)
    if (!path.startsWith(this.workspacePath)) {
      return;
    }

    // Coalesce: if the same path is already queued, update its type.
    // Deleted takes precedence over Created/Modified.
    const existing = this.eventQueue.find((e) => e.path === path);
    if (existing) {
      if (changeType === 'Deleted') {
        existing.changeType = 'Deleted';
      }
      // For Created + Modified → keep Created
      return;
    }

    this.eventQueue.push({ changeType, path });

    // Schedule flush if not already scheduled
    if (!this.flushTimer) {
      this.flushTimer = setTimeout(() => this.flush(), this.debounceMs);
    }
  }

  /**
   * Flush all queued events to the core subprocess.
   */
  private flush(): void {
    this.flushTimer = null;

    if (this.eventQueue.length === 0) {
      return;
    }

    const events = this.eventQueue.splice(0);
    logger.debug(`FileWatcher: flushing ${events.length} events`);

    if (!this.notifyCore) {
      logger.warn('FileWatcher: no notifyCore callback, dropping events');
      return;
    }

    // Send each event as a separate notification.
    // The Rust core handles debouncing internally via FILE_EVENT_DEBOUNCE_MS.
    for (const event of events) {
      this.notifyCore('project/fileEvent', {
        changeType: event.changeType,
        path: event.path,
      });
    }
  }
}
