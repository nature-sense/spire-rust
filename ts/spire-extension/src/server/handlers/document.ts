import * as vscode from 'vscode';
import type { MethodHandler } from '../router';
import type { TextDocument, Position, Range, TextEdit } from '../../model/types';

/**
 * Document handlers using real VS Code workspace/editor API.
 */
export const documentHandlers: Record<string, MethodHandler> = {
  'document/read': async (params: unknown): Promise<TextDocument> => {
    const { uri, options } = params as {
      uri: string;
      options?: { startLine?: number; endLine?: number };
    };
    const uriParsed = vscode.Uri.parse(uri);
    const doc = await vscode.workspace.openTextDocument(uriParsed);

    let text: string | undefined;
    if (options?.startLine !== undefined || options?.endLine !== undefined) {
      const startLine = options?.startLine ?? 0;
      const endLine = options?.endLine ?? doc.lineCount;
      const lines: string[] = [];
      for (let i = startLine; i < Math.min(endLine, doc.lineCount); i++) {
        lines.push(doc.lineAt(i).text);
      }
      text = lines.join('\n');
    } else {
      text = doc.getText();
    }

    return {
      uri: doc.uri.toString(),
      fileName: doc.fileName,
      languageId: doc.languageId,
      lineCount: doc.lineCount,
      isDirty: doc.isDirty,
      isUntitled: doc.isUntitled,
      text,
    };
  },

  'document/insertText': async (params: unknown): Promise<void> => {
    const { uri, position, text } = params as {
      uri: string;
      position: Position;
      text: string;
    };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = findEditor(uriParsed);
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    await editor.edit(editBuilder => {
      const pos = new vscode.Position(position.line, position.character);
      editBuilder.insert(pos, text);
    });
  },

  'document/replaceText': async (params: unknown): Promise<void> => {
    const { uri, range, text } = params as {
      uri: string;
      range: Range;
      text: string;
    };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = findEditor(uriParsed);
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    await editor.edit(editBuilder => {
      const rng = new vscode.Range(
        range.start.line, range.start.character,
        range.end.line, range.end.character
      );
      editBuilder.replace(rng, text);
    });
  },

  'document/deleteRange': async (params: unknown): Promise<void> => {
    const { uri, range } = params as {
      uri: string;
      range: Range;
    };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = findEditor(uriParsed);
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    await editor.edit(editBuilder => {
      const rng = new vscode.Range(
        range.start.line, range.start.character,
        range.end.line, range.end.character
      );
      editBuilder.delete(rng);
    });
  },

  'document/format': async (params: unknown): Promise<void> => {
    const { uri } = params as { uri: string };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = findEditor(uriParsed);
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    await vscode.commands.executeCommand('editor.action.formatDocument', uriParsed);
  },

  'document/applyEdit': async (params: unknown): Promise<boolean> => {
    const { uri, edits } = params as {
      uri: string;
      edits: TextEdit[];
    };
    const uriParsed = vscode.Uri.parse(uri);

    const editor = findEditor(uriParsed);
    if (!editor) {
      throw Object.assign(new Error(`Editor not found for ${uri}`), { code: -32001 });
    }

    return await editor.edit(editBuilder => {
      for (const edit of edits) {
        const rng = new vscode.Range(
          edit.range.start.line, edit.range.start.character,
          edit.range.end.line, edit.range.end.character
        );
        editBuilder.replace(rng, edit.newText);
      }
    });
  },
};

/**
 * Find the editor for a given URI among visible editors.
 */
function findEditor(uri: vscode.Uri): vscode.TextEditor | undefined {
  return vscode.window.visibleTextEditors.find(
    e => e.document.uri.toString() === uri.toString()
  );
}
