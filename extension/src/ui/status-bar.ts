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

export class StatusBarManager implements vscode.Disposable {
    private statusBarItem: vscode.StatusBarItem;

    constructor() {
        this.statusBarItem = vscode.window.createStatusBarItem(
            vscode.StatusBarAlignment.Left,
            100
        );
        this.statusBarItem.command = 'spire.openChat';
        this.statusBarItem.tooltip = 'Open Spire Chat';
        this.statusBarItem.show();
    }

    updateStatus(text: string, color: 'green' | 'yellow' | 'red' = 'green'): void {
        this.statusBarItem.text = text;

        const icon = color === 'green' ? '$(check)' : color === 'yellow' ? '$(sync~spin)' : '$(error)';
        this.statusBarItem.text = `${icon} ${text}`;

        const colors = {
            green: '#4ec9b0',
            yellow: '#dcdcaa',
            red: '#f44747',
        };
        this.statusBarItem.color = colors[color];
    }

    dispose(): void {
        this.statusBarItem.dispose();
    }
}
