import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { Diagnostic, Range, Position } from '../../model/types';

/**
 * Convert VS Code DiagnosticSeverity to our model severity string.
 */
function toModelSeverity(severity: vscode.DiagnosticSeverity): Diagnostic['severity'] {
  switch (severity) {
    case vscode.DiagnosticSeverity.Error: return 'error';
    case vscode.DiagnosticSeverity.Warning: return 'warning';
    case vscode.DiagnosticSeverity.Information: return 'information';
    case vscode.DiagnosticSeverity.Hint: return 'hint';
    default: return 'error';
  }
}

/**
 * Convert VS Code Diagnostic to our model Diagnostic.
 */
function toModelDiagnostic(
  uri: vscode.Uri,
  diag: vscode.Diagnostic
): Diagnostic {
  return {
    uri: uri.toString(),
    range: {
      start: { line: diag.range.start.line, character: diag.range.start.character },
      end: { line: diag.range.end.line, character: diag.range.end.character },
    },
    severity: toModelSeverity(diag.severity),
    message: diag.message,
    source: diag.source,
    code: typeof diag.code === 'string' ? diag.code : String(diag.code ?? ''),
  };
}

/**
 * Diagnostics handlers using real VS Code languages API.
 */
export const diagnosticsHandlers: Record<string, MethodHandler> = {
  'diagnostics/get': async (params: unknown): Promise<Diagnostic[]> => {
    const filter = params as { uri?: string; severity?: Diagnostic['severity'] } | undefined;

    const allDiagnostics = vscode.languages.getDiagnostics();
    const results: Diagnostic[] = [];

    for (const [uri, diags] of allDiagnostics) {
      // Filter by URI if specified
      if (filter?.uri && uri.toString() !== filter.uri) {
        continue;
      }

      for (const diag of diags) {
        const modelDiag = toModelDiagnostic(uri, diag);

        // Filter by severity if specified
        if (filter?.severity && modelDiag.severity !== filter.severity) {
          continue;
        }

        results.push(modelDiag);
      }
    }

    return results;
  },
};
