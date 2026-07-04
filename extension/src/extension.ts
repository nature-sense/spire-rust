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
import { StatusBarManager } from './ui/status-bar';
import { registerCommands } from './commands';

let mcpClient: McpClient | undefined;
let chatPanel: ChatPanel | undefined;
let statusBar: StatusBarManager | undefined;

export function activate(context: vscode.ExtensionContext) {
    console.log('Spire: activating extension...');

    // Initialize status bar
    statusBar = new StatusBarManager();
    statusBar.updateStatus('Spire: Starting...', 'yellow');
    context.subscriptions.push(statusBar);

    // Initialize MCP client (spawns Rust process)
    mcpClient = new McpClient();
    mcpClient.on('ready', () => {
        statusBar?.updateStatus('Spire: Ready', 'green');
    });
    mcpClient.on('error', (err: Error) => {
        statusBar?.updateStatus('Spire: Error', 'red');
        vscode.window.showErrorMessage(`Spire error: ${err.message}`);
    });
    mcpClient.on('progress', (update: { message: string; percent?: number }) => {
        statusBar?.updateStatus(`Spire: ${update.message}`, 'yellow');
        chatPanel?.showProgress(update);
    });

    // Initialize chat panel
    chatPanel = new ChatPanel(context.extensionUri);
    context.subscriptions.push(chatPanel);

    // Register commands
    registerCommands(context, mcpClient, chatPanel);

    // Connect to Rust MCP server
    mcpClient.connect().catch((err: Error) => {
        vscode.window.showErrorMessage(`Failed to connect to Spire: ${err.message}`);
    });
}

export function deactivate() {
    console.log('Spire: deactivating extension...');
    mcpClient?.disconnect();
    chatPanel?.dispose();
    statusBar?.dispose();
}
