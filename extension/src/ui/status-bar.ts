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
