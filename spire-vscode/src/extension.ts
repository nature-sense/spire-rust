import * as vscode from 'vscode';
import { McpClient } from './mcp/client';
import { ChatService } from './services/chat';
import { ConfigService } from './services/config';
import { ChatWebViewProvider } from './webviews/chat';
import { ConfigWebViewProvider } from './webviews/config';

let client: McpClient;
let chatService: ChatService;
let configService: ConfigService;
let chatWebView: ChatWebViewProvider;
let configWebView: ConfigWebViewProvider;

export async function activate(context: vscode.ExtensionContext) {
  console.log('Spire extension activating...');

  // Get configuration
  const config = vscode.workspace.getConfiguration('spire');
  const corePath = config.get<string>('corePath', '');

  if (!corePath) {
    const result = await vscode.window.showErrorMessage(
      'Spire core binary not configured. Please set the path in settings.',
      'Configure Now'
    );
    if (result === 'Configure Now') {
      await vscode.commands.executeCommand('workbench.action.openSettings', 'spire.corePath');
    }
    // Still start, but show error
  }

  // Initialize MCP client
  client = new McpClient();

  if (corePath) {
    try {
      await client.start(corePath);
    } catch (e) {
      vscode.window.showErrorMessage(`Failed to start Spire core: ${e}`);
    }
  }

  // Initialize services
  chatService = new ChatService(client);
  configService = new ConfigService(client);

  // Initialize WebView providers
  chatWebView = new ChatWebViewProvider(chatService);
  configWebView = new ConfigWebViewProvider(configService);

  // Register commands
  const openChat = vscode.commands.registerCommand('spire.openChat', () => {
    chatWebView.show();
  });

  const openConfig = vscode.commands.registerCommand('spire.openConfig', () => {
    configWebView.show();
  });

  const buildProject = vscode.commands.registerCommand('spire.buildProject', async () => {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders) {
      vscode.window.showErrorMessage('No workspace folder open');
      return;
    }

    chatWebView.show();
    const project = workspaceFolders[0].uri.fsPath;
    const message = `Build the project at ${project}`;
    await chatService.sendStreamingMessage(message, {
      projectRoot: project,
    });
  });

  // Register keybinding handler for chat
  const showChatQuickPick = vscode.commands.registerCommand('spire.showChatQuickPick', async () => {
    chatWebView.show();
    // Focus input after a small delay
    setTimeout(() => {
      vscode.commands.executeCommand('spire.focusChatInput');
    }, 100);
  });

  // Status bar item
  const statusBarItem = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100
  );
  statusBarItem.text = '$(hubot) Spire';
  statusBarItem.tooltip = 'Open Spire Assistant';
  statusBarItem.command = 'spire.openChat';
  statusBarItem.show();

  // Watch for config changes
  vscode.workspace.onDidChangeConfiguration((event) => {
    if (event.affectsConfiguration('spire.corePath')) {
      const newPath = vscode.workspace.getConfiguration('spire').get<string>('corePath', '');
      if (newPath) {
        client.close().then(() => {
          client.start(newPath).catch(console.error);
        });
      }
    }
  });

  context.subscriptions.push(
    openChat,
    openConfig,
    buildProject,
    showChatQuickPick,
    statusBarItem,
    client
  );

  console.log('Spire extension activated');
}

export async function deactivate() {
  console.log('Spire extension deactivating...');
  if (client) {
    await client.close();
  }
}
