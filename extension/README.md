# Spire Rust — VS Code Extension

[![VS Code](https://img.shields.io/badge/vscode-1.90%2B-blueviolet)](https://code.visualstudio.com)
[![TypeScript](https://img.shields.io/badge/typescript-5.4%2B-blue)](https://www.typescriptlang.org)

A thin VS Code extension that serves as the UI shell for the Spire Rust MCP server. It spawns the Rust binary as a child process and communicates via JSON-RPC over stdio using the Model Context Protocol (MCP).

---

## Features

- **Explain Code** (`Cmd+Shift+E` / `Ctrl+Shift+E`) — Select code in the editor and get an AI-powered explanation
- **Search Codebase** — Semantic or regex-based search across your project
- **Analyze Code** — Static analysis with complexity scoring and symbol extraction
- **Open Chat** (`Cmd+Shift+I` / `Ctrl+Shift+I`) — Webview-based chat panel showing results and progress

---

## Architecture

```
VS Code Extension (TypeScript)
│
├── extension.ts          # Lifecycle: activate/deactivate
│   ├── Spawns Rust binary as child process
│   ├── Initializes StatusBarManager
│   ├── Creates ChatPanel (webview)
│   └── Registers commands
│
├── mcp-client.ts         # JSON-RPC MCP client over stdio
│   ├── Spawns & manages Rust process
│   ├── Sends JSON-RPC requests
│   ├── Parses responses & notifications
│   └── 30-second request timeout
│
├── commands.ts           # VS Code command registrations
│   ├── spire.explainCode
│   ├── spire.searchCodebase
│   ├── spire.analyzeCode
│   └── spire.openChat
│
└── ui/
    ├── chat-panel.ts     # Webview chat interface
    │   ├── Lazy-loaded webview panel
    │   ├── Markdown message rendering
    │   └── Progress bar display
    └── status-bar.ts     # Status bar indicator
        ├── Green: Ready
        ├── Yellow: Working
        └── Red: Error
```

### Communication Flow

```
┌─────────────────────┐         JSON-RPC 2.0          ┌─────────────────────┐
│  VS Code Extension  │  ──────────────────────────►  │  Rust MCP Server    │
│  (TypeScript)       │  ◄──────────────────────────  │  (Native Binary)    │
│                     │       over stdio (stdin/stdout)│                     │
│  mcp-client.ts      │                                │  main.rs            │
└─────────────────────┘                                └─────────────────────┘
```

The extension spawns the Rust binary on activation and communicates using newline-delimited JSON messages. The Rust process writes logs to stderr (visible in the VS Code console).

---

## Prerequisites

- **VS Code** 1.90+
- **Node.js** 20+
- **npm** or **pnpm**
- **Rust** 1.75+ (for building the binary)

---

## Build & Development

```bash
# Install dependencies
cd extension
npm install

# Compile TypeScript
npm run compile

# Watch mode
npm run watch

# Run tests
npm run test
```

### Building from the Project Root

```bash
# From the repository root — builds Rust + TypeScript
pnpm run build

# Development mode — builds and launches VS Code debug session
pnpm run dev

# Package as .vsix
pnpm run package
```

---

## Binary Resolution

The extension looks for the Rust binary in the following locations (in order):

1. `core/target/release/spire-rust` (development)
2. `extension/bin/spire-rust` (production/packaged)

The `pnpm run build` script copies the release binary from `core/target/release/` to `extension/bin/`.

---

## Extension Commands

| Command | ID | Keybinding (macOS) | Keybinding (Windows/Linux) |
|---------|----|--------------------|---------------------------|
| Explain Code | `spire.explainCode` | `Cmd+Shift+E` | `Ctrl+Shift+E` |
| Open Chat | `spire.openChat` | `Cmd+Shift+I` | `Ctrl+Shift+I` |
| Search Codebase | `spire.searchCodebase` | — | — |
| Analyze Code | `spire.analyzeCode` | — | — |

---

## Extension Details

### Activation

The extension activates on `onStartupFinished` — it starts automatically once VS Code is ready. On activation:

1. Creates the status bar indicator
2. Initializes the MCP client (spawns the Rust binary)
3. Creates the chat panel (lazy — shown on first command)
4. Registers all commands
5. Connects to the Rust MCP server

### Status Bar

The status bar shows the current state of the Spire backend:

| State | Icon | Color |
|-------|------|-------|
| Starting | `$(sync~spin)` | Yellow |
| Ready | `$(check)` | Green |
| Error | `$(error)` | Red |

### Chat Panel

The webview-based chat panel displays:
- Formatted messages (Markdown support)
- Progress indicators with percentage bars
- Status updates from the Rust backend

---

## Packaging

```bash
# Package the extension as a .vsix file
pnpm run package

# The .vsix will be created in the extension/ directory
```

The extension is published under the publisher name `naturesense`.

---

## Dependencies

| Package | Version | Purpose |
|---------|---------|---------|
| `@types/vscode` | ^1.90.0 | VS Code API type definitions |
| `@types/node` | ^26.0.1 | Node.js type definitions |
| `typescript` | ^5.4.0 | TypeScript compiler |
| `vsce` | ^2.15.0 | VS Code extension packaging tool |

---

## Development Notes

### Adding a New Command

1. Add the command to `contributes.commands` in `package.json`
2. Add an optional keybinding in `contributes.keybindings`
3. Implement the command handler in `commands.ts`
4. Register it in the `registerCommands` function

### Adding a New MCP Tool

1. Define the tool in `core/src/mcp/tools.rs`
2. Add a corresponding command in `extension/src/commands.ts`
3. Call `mcpClient.sendRequest('tools/call', { name: 'tool_name', arguments: {...} })`

### Debugging

The `.vscode/launch.json` and `.vscode/tasks.json` files at the repository root provide pre-configured debug configurations. The Rust process logs to stderr, which appears in the VS Code console (`Developer: Toggle Developer Tools`).

---

## License

GNU GPLv3 — see [LICENSE](../LICENSE) for details.
