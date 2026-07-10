import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { Symbol, Position, HoverInfo, Range } from '../../model/types';

/**
 * Convert VS Code SymbolKind to our model kind string.
 */
function toModelKind(kind: vscode.SymbolKind): Symbol['kind'] {
  switch (kind) {
    case vscode.SymbolKind.Function: return 'function';
    case vscode.SymbolKind.Class: return 'class';
    case vscode.SymbolKind.Variable: return 'variable';
    case vscode.SymbolKind.Method: return 'method';
    case vscode.SymbolKind.Interface: return 'interface';
    case vscode.SymbolKind.Enum: return 'enum';
    case vscode.SymbolKind.Module: return 'module';
    case vscode.SymbolKind.Property: return 'property';
    case vscode.SymbolKind.Constant: return 'constant';
    default: return 'variable';
  }
}

/**
 * Convert VS Code Range to our model Range.
 */
function toModelRange(range: vscode.Range): Range {
  return {
    start: { line: range.start.line, character: range.start.character },
    end: { line: range.end.line, character: range.end.character },
  };
}

/**
 * Convert VS Code Location to our model Symbol.
 */
function toModelSymbol(
  name: string,
  kind: vscode.SymbolKind,
  location: vscode.Location,
  containerName?: string
): Symbol {
  return {
    name,
    kind: toModelKind(kind),
    uri: location.uri.toString(),
    range: toModelRange(location.range),
    selectionRange: toModelRange(location.range),
    containerName,
  };
}

/**
 * Symbol / code intelligence handlers using real VS Code language features API.
 */
export const symbolHandlers: Record<string, MethodHandler> = {
  'symbols/goToDefinition': async (params: unknown): Promise<Symbol | null> => {
    const { uri, position } = params as { uri: string; position: Position };
    const uriParsed = vscode.Uri.parse(uri);
    const pos = new vscode.Position(position.line, position.character);

    try {
      const definitions = await vscode.commands.executeCommand<vscode.Location[]>(
        'vscode.executeDefinitionProvider',
        uriParsed,
        pos
      );

      if (!definitions || definitions.length === 0) return null;

      const def = definitions[0];
      return toModelSymbol(
        def.uri.fsPath.split('/').pop() ?? 'definition',
        vscode.SymbolKind.Function,
        def
      );
    } catch {
      return null;
    }
  },

  'symbols/findReferences': async (params: unknown): Promise<Symbol[]> => {
    const { uri, position } = params as { uri: string; position: Position };
    const uriParsed = vscode.Uri.parse(uri);
    const pos = new vscode.Position(position.line, position.character);

    try {
      const references = await vscode.commands.executeCommand<vscode.Location[]>(
        'vscode.executeReferenceProvider',
        uriParsed,
        pos
      );

      if (!references) return [];

      return references.map((ref, i) =>
        toModelSymbol(
          `reference #${i + 1}`,
          vscode.SymbolKind.Variable,
          ref
        )
      );
    } catch {
      return [];
    }
  },

  'symbols/getHover': async (params: unknown): Promise<HoverInfo | null> => {
    const { uri, position } = params as { uri: string; position: Position };
    const uriParsed = vscode.Uri.parse(uri);
    const pos = new vscode.Position(position.line, position.character);

    try {
      const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
        'vscode.executeHoverProvider',
        uriParsed,
        pos
      );

      if (!hovers || hovers.length === 0) return null;

      const hover = hovers[0];
      const contents = hover.contents
        .map(content => {
          if (typeof content === 'string') return content;
          if ('value' in content) return (content as { value: string }).value;
          return '';
        })
        .join('\n');

      return {
        contents,
        range: hover.range ? toModelRange(hover.range) : undefined,
      };
    } catch {
      return null;
    }
  },
};
