import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { Terminal } from '../../model/types';

/**
 * Terminal handlers using real VS Code terminal API.
 */
export const terminalHandlers: Record<string, MethodHandler> = {
  'terminal/list': async (): Promise<Terminal[]> => {
    return vscode.window.terminals.map(t => {
      const opts = t.creationOptions as { shellPath?: { toString(): string } } | undefined;
      return {
        id: t.name,
        name: t.name,
        shellPath: opts?.shellPath?.toString() ?? '/bin/zsh',
        isVisible: t.exitStatus === undefined, // still running = visible
      };
    });
  },

  'terminal/create': async (params: unknown): Promise<string> => {
    const { name, options } = params as {
      name: string;
      options?: { cwd?: string; env?: Record<string, string> };
    };

    const terminal = vscode.window.createTerminal({
      name,
      cwd: options?.cwd,
      env: options?.env,
    });

    terminal.show();
    return name; // VS Code doesn't expose terminal ID, use name as identifier
  },

  'terminal/send': async (params: unknown): Promise<void> => {
    const { terminalId, text, options } = params as {
      terminalId: string;
      text: string;
      options?: { addNewline?: boolean };
    };

    const terminal = vscode.window.terminals.find(t => t.name === terminalId);
    if (!terminal) {
      throw Object.assign(new Error(`Terminal not found: ${terminalId}`), { code: -32001 });
    }

    terminal.show();
    terminal.sendText(text, options?.addNewline ?? true);
  },

  'terminal/dispose': async (params: unknown): Promise<void> => {
    const { terminalId } = params as { terminalId: string };

    const terminal = vscode.window.terminals.find(t => t.name === terminalId);
    if (terminal) {
      terminal.dispose();
    }
  },
};
