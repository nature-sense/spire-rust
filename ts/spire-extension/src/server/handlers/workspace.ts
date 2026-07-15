import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { WorkspaceFolder, SearchMatch } from '../../model/types';

/**
 * Workspace handlers using real VS Code workspace API.
 */
export const workspaceHandlers: Record<string, MethodHandler> = {
  'workspace/getFolders': async (): Promise<WorkspaceFolder[]> => {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders) return [];
    return folders.map(f => ({
      name: f.name,
      uri: f.uri.toString(),
      isActive: true,
    }));
  },

  'workspace/searchFiles': async (params: unknown): Promise<string[]> => {
    const { pattern, options } = params as {
      pattern: string;
      options?: { include?: string; exclude?: string };
    };
    const exclude = options?.exclude
      ? options.exclude.split(',').map(s => s.trim()).filter(Boolean)
      : undefined;
    const uris = await vscode.workspace.findFiles(
      pattern,
      exclude ? `{${exclude.join(',')}}` : undefined
    );
    return uris.map(uri => uri.toString());
  },

  'workspace/searchText': async (params: unknown): Promise<SearchMatch[]> => {
    const { pattern, options } = params as {
      pattern: string;
      options?: { include?: string; maxResults?: number; contextLines?: number };
    };
    const maxResults = options?.maxResults ?? 50;
    const results: SearchMatch[] = [];

    const regex = new RegExp(pattern, 'gi');

    // Use findFiles to get candidate files, then read and search each
    const filePattern = options?.include ?? '**/*';
    const uris = await vscode.workspace.findFiles(filePattern, undefined, maxResults);

    for (const uri of uris) {
      try {
        const doc = await vscode.workspace.openTextDocument(uri);
        const text = doc.getText();
        const lines = text.split('\n');

        for (let i = 0; i < lines.length; i++) {
          const match = regex.exec(lines[i]);
          if (match) {
            const context: string[] = [];
            if (options?.contextLines) {
              const start = Math.max(0, i - options.contextLines);
              const end = Math.min(lines.length - 1, i + options.contextLines);
              for (let j = start; j <= end; j++) {
                context.push(lines[j]);
              }
            }
            results.push({
              uri: uri.toString(),
              line: i + 1, // 1-based for display
              column: match.index + 1,
              lineContent: lines[i],
              context: context.length > 0 ? context : undefined,
            });

            if (results.length >= maxResults) break;
          }
          regex.lastIndex = 0; // Reset for each line
        }
      } catch {
        // Skip files that can't be read
        continue;
      }
      if (results.length >= maxResults) break;
    }

    return results.slice(0, maxResults);
  },
};
