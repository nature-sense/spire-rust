import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { TextEditor, TextDocument, Position, Range } from '../../model/types';
import type { Selection as ModelSelection } from '../../model/types';

/**
 * Convert VS Code Position to our model Position.
 */
function toModelPosition(pos: vscode.Position): Position {
  return { line: pos.line, character: pos.character };
}

/**
 * Convert VS Code Range to our model Range.
 */
function toModelRange(range: vscode.Range): Range {
  return {
    start: toModelPosition(range.start),
    end: toModelPosition(range.end),
  };
}

/**
 * Convert VS Code Selection to our model Selection.
 */
function toModelSelection(sel: vscode.Selection): ModelSelection {
  // VS Code Selection doesn't have a .text property directly;
  // we compute it from the document text if needed
  return {
    start: toModelPosition(sel.start),
    end: toModelPosition(sel.end),
    isEmpty: sel.isEmpty,
  };
}

/**
 * Convert VS Code TextDocument to our model TextDocument.
 */
function toModelDocument(doc: vscode.TextDocument): TextDocument {
  return {
    uri: doc.uri.toString(),
    fileName: doc.fileName,
    languageId: doc.languageId,
    lineCount: doc.lineCount,
    isDirty: doc.isDirty,
    isUntitled: doc.isUntitled,
  };
}

/**
 * Convert VS Code TextEditor to our model TextEditor.
 */
function toModelEditor(editor: vscode.TextEditor): TextEditor {
  return {
    document: toModelDocument(editor.document),
    viewColumn: editor.viewColumn ?? 1,
    selections: editor.selections.map(toModelSelection),
    visibleRanges: editor.visibleRanges.map(toModelRange),
  };
}

/**
 * Editor handlers using real VS Code window API.
 */
export const editorHandlers: Record<string, MethodHandler> = {
  'editor/getActive': async (): Promise<TextEditor | null> => {
    const editor = vscode.window.activeTextEditor;
    if (!editor) return null;
    return toModelEditor(editor);
  },

  'editor/getVisible': async (): Promise<TextEditor[]> => {
    return vscode.window.visibleTextEditors.map(toModelEditor);
  },

  'editor/openFile': async (params: unknown): Promise<void> => {
    const { uri, options } = params as {
      uri: string;
      options?: { line?: number; column?: number; viewColumn?: number; preview?: boolean };
    };
    const uriParsed = vscode.Uri.parse(uri);

    let selection: vscode.Range | undefined;
    if (options?.line !== undefined) {
      const line = Math.max(0, options.line - 1); // Convert 1-based to 0-based
      const col = options.column ? Math.max(0, options.column - 1) : 0;
      selection = new vscode.Range(line, col, line, col);
    }

    await vscode.window.showTextDocument(uriParsed, {
      viewColumn: options?.viewColumn,
      preview: options?.preview,
      selection,
    });
  },

  'editor/close': async (params: unknown): Promise<void> => {
    const { uri } = params as { uri: string };
    const uriParsed = vscode.Uri.parse(uri);

    // Find the tab group that has this document open
    for (const tabGroup of vscode.window.tabGroups.all) {
      for (const tab of tabGroup.tabs) {
        const input = tab.input as { uri?: vscode.Uri } | undefined;
        if (input?.uri?.toString() === uriParsed.toString()) {
          await vscode.window.tabGroups.close(tab);
          return;
        }
      }
    }
  },

  'editor/setSelection': async (params: unknown): Promise<void> => {
    const { uri, selection } = params as {
      uri: string;
      selection: { start: Position; end: Position };
    };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = vscode.window.visibleTextEditors.find(
      e => e.document.uri.toString() === uriParsed.toString()
    );
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    const start = new vscode.Position(selection.start.line, selection.start.character);
    const end = new vscode.Position(selection.end.line, selection.end.character);
    editor.selection = new vscode.Selection(start, end);
    editor.revealRange(new vscode.Range(start, end), vscode.TextEditorRevealType.Default);
  },

  'editor/revealRange': async (params: unknown): Promise<void> => {
    const { uri, range } = params as { uri: string; range: Range };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = vscode.window.visibleTextEditors.find(
      e => e.document.uri.toString() === uriParsed.toString()
    );
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    const start = new vscode.Position(range.start.line, range.start.character);
    const end = new vscode.Position(range.end.line, range.end.character);
    editor.revealRange(new vscode.Range(start, end), vscode.TextEditorRevealType.Default);
  },
};
