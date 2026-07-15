#!/usr/bin/env node

/**
 * Mock Environment Server (Bidirectional)
 *
 * A JSON-RPC 2.0 server over stdin/stdout that simulates the Rust core
 * subprocess. Supports bidirectional communication:
 *
 * 1. Handles incoming requests from the extension (chat, agent, etc.)
 * 2. Can send requests TO the extension for VS Code API operations
 *    (workspace, editor, diagnostics, etc.)
 * 3. Sends notifications (events) to the extension
 *
 * Usage:
 *   node test/mock-env-server.mjs
 */

import { createInterface } from 'node:readline';

// ── In-memory chat store ────────────────────────────────────────────────────

const chats = new Map();

function getDefaultChat() {
  for (const chat of chats.values()) return chat;
  const now = new Date().toISOString();
  const chat = {
    id: 'default',
    title: 'Mock Chat',
    messages: [],
    status: 'idle',
    createdAt: now,
    updatedAt: now,
  };
  chats.set(chat.id, chat);
  return chat;
}

// ── Pending outgoing requests (mock server → extension) ─────────────────────

let nextId = 1000;
const pending = new Map();

// ── Method handlers (handles incoming requests from extension) ──────────────

const handlers = {

  'ping': async () => 'pong',

  'chat/getActive': async () => getDefaultChat(),

  'chat/getHistory': async () => Array.from(chats.values()),

  'chat/getMessage': async (params) => {
    const { chatId, messageId } = params;
    const chat = chats.get(chatId);
    if (!chat) return null;
    return chat.messages.find(m => m.id === messageId) ?? null;
  },

  'chat/append': async (params) => {
    const { chatId, content, options } = params;
    let chat = chats.get(chatId);
    if (!chat) {
      const now = new Date().toISOString();
      chat = {
        id: chatId,
        title: 'New Chat',
        messages: [],
        status: 'idle',
        createdAt: now,
        updatedAt: now,
      };
      chats.set(chatId, chat);
    }

    const message = {
      id: `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      role: options?.role ?? 'assistant',
      content,
      timestamp: new Date().toISOString(),
      metadata: options?.metadata ?? null,
    };

    chat.messages.push(message);
    chat.updatedAt = message.timestamp;

    // If user sent a message, simulate an assistant reply after a short delay
    if (options?.role === 'user') {
      simulateAssistantReply(chatId, content);
    }

    return message;
  },

  'chat/clear': async (params) => {
    const { chatId } = params;
    const chat = chats.get(chatId);
    if (chat) {
      chat.messages = [];
      chat.updatedAt = new Date().toISOString();
    }
  },

  'chat/setTitle': async (params) => {
    const { chatId, title } = params;
    const chat = chats.get(chatId) || getDefaultChat();
    chat.title = title;
    chat.updatedAt = new Date().toISOString();
  },

  // The mock server also handles VS Code API methods locally for testing
  // In production, these would be sent TO the extension via bidirectional channel
  'workspace/getFolders': async () => {
    return [
      { name: 'spire-rust', uri: 'file:///Users/steve/naturesense/tools/spire-rust', isActive: true },
    ];
  },

  'workspace/searchFiles': async (params) => {
    const { pattern, options } = params;
    return [
      'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
      'file:///Users/steve/naturesense/tools/spire-rust/src/lib.rs',
    ];
  },

  'workspace/searchText': async (params) => {
    const { pattern, options } = params;
    return [
      { uri: 'file:///test.ts', line: 1, column: 1, lineText: `import { something } from 'module'`, matchLength: 6 },
      { uri: 'file:///test.ts', line: 5, column: 1, lineText: `import { other } from 'other'`, matchLength: 6 },
    ];
  },

  'editor/getActive': async () => {
    return {
      document: {
        uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
        fileName: 'main.rs',
        languageId: 'rust',
        lineCount: 120,
        isDirty: false,
        isUntitled: false,
      },
      viewColumn: 1,
      selections: [],
      visibleRanges: [],
    };
  },

  'editor/getVisible': async () => {
    return [
      {
        document: {
          uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
          fileName: 'main.rs',
          languageId: 'rust',
          lineCount: 120,
          isDirty: false,
          isUntitled: false,
        },
        viewColumn: 1,
        selections: [],
        visibleRanges: [],
      },
    ];
  },

  'diagnostics/get': async (params) => {
    const filter = params || {};
    return [
      {
        uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
        range: { start: { line: 10, character: 0 }, end: { line: 10, character: 10 } },
        severity: 'error',
        message: 'Mock diagnostic error',
        source: 'mock',
        code: 'E001',
      },
    ];
  },

  'terminal/list': async () => {
    return [
      { id: 'term-1', name: 'zsh', shellPath: '/bin/zsh', isVisible: true },
    ];
  },

  'terminal/create': async (params) => {
    const { name } = params;
    return name;
  },

  'terminal/send': async (params) => {
    const { terminalId, text, options } = params;
    // Mock: just acknowledge
  },

  'terminal/dispose': async (params) => {
    const { terminalId } = params;
    // Mock: just acknowledge
  },

  'git/getChanges': async (params) => {
    const filter = params || {};
    return [
      {
        uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
        status: 'modified',
        staged: false,
      },
    ];
  },

  'symbols/goToDefinition': async (params) => {
    const { uri, position } = params;
    return {
      name: 'main',
      kind: 'function',
      uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/main.rs',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
      selectionRange: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
    };
  },

  'symbols/findReferences': async (params) => {
    const { uri, position } = params;
    return [
      {
        name: 'reference #1',
        kind: 'variable',
        uri: 'file:///Users/steve/naturesense/tools/spire-rust/src/lib.rs',
        range: { start: { line: 5, character: 0 }, end: { line: 5, character: 4 } },
        selectionRange: { start: { line: 5, character: 0 }, end: { line: 5, character: 4 } },
      },
    ];
  },

  'symbols/getHover': async (params) => {
    const { uri, position } = params;
    return {
      contents: '```rust\nfn main() -> i32\n```\nThe entry point of the program.',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 4 } },
    };
  },

  'document/read': async (params) => {
    const { uri, options } = params;
    return {
      uri,
      fileName: uri.split('/').pop(),
      languageId: 'typescript',
      lineCount: 100,
      isDirty: false,
      isUntitled: false,
      text: '// Mock document content\nconst x = 1;\n',
    };
  },

  // ── MCP handlers ──────────────────────────────────────────────────────────

  'mcp/listServers': async () => {
    return [
      {
        name: 'core',
        description: 'Built-in system tools for Spire core operations',
        server_type: 'embedded',
        tool_count: 3,
        properties: { status: 'online' },
      },
      {
        name: 'git',
        description: 'Git version control operations (commit, diff, log)',
        server_type: 'external',
        tool_count: 2,
        properties: {
          status: 'online',
          command: 'node',
          args: ['mcp/mcp-git/dist/index.js'],
        },
      },
      {
        name: 'filesystem',
        description: 'File system read/write operations',
        server_type: 'external',
        tool_count: 3,
        properties: {
          status: 'online',
          command: 'node',
          args: ['mcp/mcp-fs/dist/index.js'],
        },
      },
      {
        name: 'search',
        description: 'Code search and text search across the workspace',
        server_type: 'external',
        tool_count: 2,
        properties: {
          status: 'offline',
          command: 'node',
          args: ['mcp/mcp-search/dist/index.js'],
        },
      },
    ];
  },

  'mcp/listServerTools': async (params) => {
    const { serverName } = params;

    const toolMap = {
      'core': [
        {
          name: 'list_mcp_servers',
          description: 'List all registered MCP servers and their status',
          input_schema: { type: 'object', properties: {} },
          enabled: true,
        },
        {
          name: 'list_mcp_tools',
          description: 'List all tools for a given MCP server',
          input_schema: {
            type: 'object',
            properties: {
              server_name: { type: 'string', description: 'Name of the MCP server' },
            },
            required: ['server_name'],
          },
          enabled: true,
        },
        {
          name: 'echo',
          description: 'Echo the input back as a response',
          input_schema: {
            type: 'object',
            properties: {
              text: { type: 'string', description: 'Text to echo back' },
            },
            required: ['text'],
          },
          enabled: true,
        },
      ],
      'git': [
        {
          name: 'git_status',
          description: 'Show the working tree status',
          input_schema: { type: 'object', properties: {} },
          enabled: true,
        },
        {
          name: 'git_log',
          description: 'Show commit logs',
          input_schema: {
            type: 'object',
            properties: {
              max_count: { type: 'number', description: 'Maximum number of commits to show' },
              path: { type: 'string', description: 'Show commits that touch this path' },
            },
          },
          enabled: true,
        },
      ],
      'filesystem': [
        {
          name: 'read_file',
          description: 'Read the contents of a file',
          input_schema: {
            type: 'object',
            properties: {
              path: { type: 'string', description: 'Absolute path to the file' },
            },
            required: ['path'],
          },
          enabled: true,
        },
        {
          name: 'write_file',
          description: 'Write content to a file',
          input_schema: {
            type: 'object',
            properties: {
              path: { type: 'string', description: 'Absolute path to the file' },
              content: { type: 'string', description: 'Content to write' },
            },
            required: ['path', 'content'],
          },
          enabled: false,
        },
        {
          name: 'list_directory',
          description: 'List files and directories in a path',
          input_schema: {
            type: 'object',
            properties: {
              path: { type: 'string', description: 'Absolute path to the directory' },
              recursive: { type: 'boolean', description: 'List recursively' },
            },
            required: ['path'],
          },
          enabled: true,
        },
      ],
      'search': [
        {
          name: 'search_files',
          description: 'Search for files by glob pattern',
          input_schema: {
            type: 'object',
            properties: {
              pattern: { type: 'string', description: 'Glob pattern to match' },
            },
            required: ['pattern'],
          },
          enabled: true,
        },
        {
          name: 'search_text',
          description: 'Search for text across files using regex',
          input_schema: {
            type: 'object',
            properties: {
              pattern: { type: 'string', description: 'Regex pattern to search for' },
              file_pattern: { type: 'string', description: 'Optional file glob filter' },
            },
            required: ['pattern'],
          },
          enabled: true,
        },
      ],
    };

    return toolMap[serverName] || [];
  },

  // ── Tools handlers ─────────────────────────────────────────────────────────

  'tools/listAll': async () => {
    // Aggregate all tools from all MCP servers + internal tools
    const internalTools = [
      {
        name: 'list_mcp_servers',
        description: 'List all registered MCP servers and their status',
        mcp_name: 'core',
      },
      {
        name: 'list_mcp_tools',
        description: 'List all tools for a given MCP server',
        mcp_name: 'core',
      },
      {
        name: 'echo',
        description: 'Echo the input back as a response',
        mcp_name: 'core',
      },
      {
        name: 'git_status',
        description: 'Show the working tree status',
        mcp_name: 'git',
      },
      {
        name: 'git_log',
        description: 'Show commit logs',
        mcp_name: 'git',
      },
      {
        name: 'read_file',
        description: 'Read the contents of a file',
        mcp_name: 'filesystem',
      },
      {
        name: 'write_file',
        description: 'Write content to a file',
        mcp_name: 'filesystem',
      },
      {
        name: 'list_directory',
        description: 'List files and directories in a path',
        mcp_name: 'filesystem',
      },
      {
        name: 'search_files',
        description: 'Search for files by glob pattern',
        mcp_name: 'search',
      },
      {
        name: 'search_text',
        description: 'Search for text across files using regex',
        mcp_name: 'search',
      },
      {
        name: 'vscode_open_file',
        description: 'Open a file in the VS Code editor',
        mcp_name: 'vscode-extension',
      },
      {
        name: 'vscode_apply_edit',
        description: 'Apply text edits to a document',
        mcp_name: 'vscode-extension',
      },
      {
        name: 'vscode_show_message',
        description: 'Show an information message to the user',
        mcp_name: 'vscode-extension',
      },
      {
        name: 'vscode_execute_command',
        description: 'Execute a VS Code command',
        mcp_name: 'vscode-extension',
      },
      {
        name: 'config_get',
        description: 'Get a configuration value',
        mcp_name: 'internal',
      },
      {
        name: 'config_set',
        description: 'Set a configuration value',
        mcp_name: 'internal',
      },
      {
        name: 'memory_search',
        description: 'Search the memory graph for related context',
        mcp_name: 'internal',
      },
      {
        name: 'memory_store',
        description: 'Store information in the memory graph',
        mcp_name: 'internal',
      },
    ];

    return internalTools;
  },
};

// ── Simulate assistant reply ────────────────────────────────────────────────

function simulateAssistantReply(chatId, userMessage) {
  const delay = 300 + Math.random() * 200; // 300-500ms
  setTimeout(() => {
    const chat = chats.get(chatId);
    if (!chat) return;

    const reply = {
      id: `msg-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      role: 'assistant',
      content: `You said: "${userMessage.slice(0, 50)}${userMessage.length > 50 ? '...' : ''}"\n\nI'm a mock server. Here's a simulated response.`,
      timestamp: new Date().toISOString(),
    };

    chat.messages.push(reply);
    chat.updatedAt = reply.timestamp;

    // Push notification to client
    sendNotification('event/chat/message', { chatId, message: reply });
  }, delay);
}

// ── Bidirectional: Send request TO extension ────────────────────────────────

/**
 * Send a request to the extension and wait for a response.
 * This simulates the Rust subprocess calling VS Code API methods.
 */
function callExtension(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = nextId++;
    const request = JSON.stringify({
      jsonrpc: '2.0',
      id,
      method,
      params,
    });

    const timer = setTimeout(() => {
      pending.delete(id);
      reject(new Error(`Timeout: callExtension(${method})`));
    }, 5000);

    pending.set(id, { resolve, reject, timer });
    process.stdout.write(request + '\n');
  });
}

// ── JSON-RPC dispatcher ─────────────────────────────────────────────────────

function sendResponse(id, result) {
  const response = JSON.stringify({ jsonrpc: '2.0', id, result });
  process.stdout.write(response + '\n');
}

function sendError(id, code, message, data = null) {
  const error = { code, message };
  if (data !== null) error.data = data;
  const response = JSON.stringify({ jsonrpc: '2.0', id, error });
  process.stdout.write(response + '\n');
}

function sendNotification(method, params) {
  const notification = JSON.stringify({
    jsonrpc: '2.0',
    method,
    params,
  });
  process.stdout.write(notification + '\n');
}

function handleRequest(request) {
  // Validate basic structure
  if (!request || request.jsonrpc !== '2.0') {
    sendError(-32600, 'Invalid Request: missing or invalid jsonrpc field');
    return;
  }

  const { id, method, params } = request;

  // Notification (no id) — just acknowledge silently
  if (id === undefined || id === null) {
    return;
  }

  if (!method || typeof method !== 'string') {
    sendError(id, -32600, 'Invalid Request: missing method');
    return;
  }

  const handler = handlers[method];
  if (!handler) {
    sendError(id, -32601, `Method not found: ${method}`);
    return;
  }

  // Call handler and send response
  Promise.resolve()
    .then(() => handler(params ?? {}))
    .then((result) => sendResponse(id, result))
    .catch((err) => sendError(id, -32603, `Internal error: ${err.message}`));
}

// ── Main ────────────────────────────────────────────────────────────────────

// Handle SIGTERM gracefully
process.on('SIGTERM', () => {
  process.exit(0);
});

process.on('SIGINT', () => {
  process.exit(0);
});

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });

rl.on('line', (line) => {
  try {
    const msg = JSON.parse(line);

    // Check if this is a response to one of our outgoing requests
    if (msg.id !== undefined && msg.id !== null && pending.has(msg.id)) {
      const p = pending.get(msg.id);
      clearTimeout(p.timer);
      pending.delete(msg.id);
      if (msg.error) {
        p.reject(new Error(msg.error.message));
      } else {
        p.resolve(msg.result);
      }
      return;
    }

    // Otherwise, it's an incoming request from the extension
    handleRequest(msg);
  } catch (err) {
    // Parse error
    const errorResponse = JSON.stringify({
      jsonrpc: '2.0',
      id: -1,
      error: { code: -32700, message: `Parse error: ${err.message}` },
    });
    process.stdout.write(errorResponse + '\n');
  }
});

// Signal readiness
process.stderr.write('Mock environment server ready\n');
