// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

import * as vscode from 'vscode';
import { McpClient } from './mcp-client';
import { ChatPanel } from './ui/chat-panel';

export function registerCommands(
    context: vscode.ExtensionContext,
    mcpClient: McpClient,
    chatPanel: ChatPanel
): void {
    context.subscriptions.push(
        vscode.commands.registerCommand('spire.explainCode', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('No active editor to explain code from.');
                return;
            }

            const selection = editor.selection;
            const selectedText = editor.document.getText(selection);

            if (!selectedText) {
                vscode.window.showWarningMessage('Select some code to explain.');
                return;
            }

            chatPanel.show();
            chatPanel.showMessage(`**Explaining selected code...**\n\`\`\`\n${selectedText}\n\`\`\``);

            try {
                const result = await mcpClient.sendRequest('tools/call', {
                    name: 'explain_code',
                    arguments: {
                        code: selectedText,
                        language: editor.document.languageId,
                    },
                });
                const response = result as { content?: Array<{ text?: string }> };
                const explanation = response?.content?.[0]?.text || 'No explanation returned.';
                chatPanel.showMessage(explanation);
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                vscode.window.showErrorMessage(`Spire explain failed: ${message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('spire.searchCodebase', async () => {
            const query = await vscode.window.showInputBox({
                prompt: 'Enter a search query for the codebase',
                placeHolder: 'e.g., "find all places where we handle authentication"',
            });

            if (!query) return;

            chatPanel.show();
            chatPanel.showMessage(`**Searching codebase for:** ${query}`);

            try {
                const result = await mcpClient.sendRequest('tools/call', {
                    name: 'search_codebase',
                    arguments: { query },
                });
                const response = result as { content?: Array<{ text?: string }> };
                const searchResults = response?.content?.[0]?.text || 'No results found.';
                chatPanel.showMessage(searchResults);
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                vscode.window.showErrorMessage(`Spire search failed: ${message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('spire.analyzeCode', async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                vscode.window.showWarningMessage('No active editor to analyze.');
                return;
            }

            const document = editor.document;
            const filePath = document.uri.fsPath;
            const fullText = document.getText();

            chatPanel.show();
            chatPanel.showMessage(`**Analyzing:** \`${filePath}\``);

            try {
                const result = await mcpClient.sendRequest('tools/call', {
                    name: 'analyze_code',
                    arguments: {
                        file_path: filePath,
                        content: fullText,
                        language: document.languageId,
                    },
                });
                const response = result as { content?: Array<{ text?: string }> };
                const analysis = response?.content?.[0]?.text || 'No analysis returned.';
                chatPanel.showMessage(analysis);
            } catch (err) {
                const message = err instanceof Error ? err.message : String(err);
                vscode.window.showErrorMessage(`Spire analysis failed: ${message}`);
            }
        })
    );

    context.subscriptions.push(
        vscode.commands.registerCommand('spire.openChat', () => {
            chatPanel.show();
        })
    );
}
