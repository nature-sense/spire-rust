import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { GitChange } from '../../model/types';

/**
 * Git handlers using VS Code's built-in Git extension API.
 *
 * The Git extension exposes its API via `vscode.extensions.getExtension('vscode.git')`.
 * We access the repository's state to get working tree changes.
 */
export const gitHandlers: Record<string, MethodHandler> = {
  'git/getChanges': async (params: unknown): Promise<GitChange[]> => {
    const filter = params as { staged?: boolean; uri?: string } | undefined;

    try {
      const gitExtension = vscode.extensions.getExtension('vscode.git');
      if (!gitExtension) {
        return [];
      }

      const gitApi = gitExtension.exports;
      const repositories = gitApi.getRepositories();
      if (!repositories || repositories.length === 0) {
        return [];
      }

      const results: GitChange[] = [];

      for (const repo of repositories) {
        const state = repo.state;

        // Working tree changes
        const workspaceChanges = state.workingTreeChanges ?? [];
        for (const change of workspaceChanges) {
          if (filter?.staged === true) continue; // not staged

          const changeUri = change.uri.toString();
          if (filter?.uri && changeUri !== filter.uri) continue;

          results.push({
            uri: changeUri,
            originalFileName: change.originalUri?.fsPath,
            status: mapGitStatus(change.status),
            staged: false,
          });
        }

        // Index changes (staged)
        const indexChanges = state.indexChanges ?? [];
        for (const change of indexChanges) {
          if (filter?.staged === false) continue; // only staged

          const changeUri = change.uri.toString();
          if (filter?.uri && changeUri !== filter.uri) continue;

          results.push({
            uri: changeUri,
            originalFileName: change.originalUri?.fsPath,
            status: mapGitStatus(change.status),
            staged: true,
          });
        }
      }

      return results;
    } catch {
      // Git extension not available or API changed
      return [];
    }
  },
};

/**
 * Map VS Code Git extension status to our model status string.
 */
function mapGitStatus(
  status: number
): GitChange['status'] {
  // VS Code Git extension status constants
  const GitStatus = {
    INDEX_MODIFIED: 1,
    INDEX_ADDED: 2,
    INDEX_DELETED: 3,
    INDEX_RENAMED: 4,
    INDEX_COPIED: 5,
    MODIFIED: 6,
    DELETED: 7,
    UNTRACKED: 8,
    IGNORED: 9,
    ADDED_BY_US: 10,
    ADDED_BY_THEM: 11,
    DELETED_BY_US: 12,
    DELETED_BY_THEM: 13,
    BOTH_ADDED: 14,
    BOTH_DELETED: 15,
    BOTH_MODIFIED: 16,
  } as const;

  switch (status) {
    case GitStatus.INDEX_ADDED:
    case GitStatus.ADDED_BY_US:
    case GitStatus.ADDED_BY_THEM:
    case GitStatus.BOTH_ADDED:
      return 'added';
    case GitStatus.INDEX_MODIFIED:
    case GitStatus.MODIFIED:
    case GitStatus.BOTH_MODIFIED:
      return 'modified';
    case GitStatus.INDEX_DELETED:
    case GitStatus.DELETED:
    case GitStatus.DELETED_BY_US:
    case GitStatus.DELETED_BY_THEM:
    case GitStatus.BOTH_DELETED:
      return 'deleted';
    case GitStatus.INDEX_RENAMED:
    case GitStatus.INDEX_COPIED:
      return 'renamed';
    default:
      return 'modified';
  }
}
