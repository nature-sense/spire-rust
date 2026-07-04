# Spire VS Code Extension

A Visual Studio Code extension that provides an AI assistant interface powered by the Spire Rust core. The extension communicates with the core binary via the Model Context Protocol (MCP) over stdio.

## Features

- **Chat Interface** — Conversational AI assistant with streaming responses, progress indicators, and artifact display
- **Configuration Editor** — Manage Spire settings (model, max steps, temperature, core path) and run agent tasks
- **Status Bar Integration** — Quick access to chat and configuration from the VS Code status bar
- **Keyboard Shortcut** — `Cmd+Shift+A` to open the chat panel

## Commands

| Command | ID | Description |
|---------|----|-------------|
| Open Chat | `spire.openChat` | Open the AI assistant chat panel |
| Open Config | `spire.openConfig` | Open the configuration editor |
| Build Project | `spire.buildProject` | Build the Spire Rust core |

## Configuration

| Setting | Default | Description |
|---------|---------|-------------|
| `spire.corePath` | `""` | Path to the Spire Rust core binary |
| `spire.model` | `"gpt-4"` | LLM model to use |
| `spire.maxSteps` | `10` | Maximum agent steps |
| `spire.temperature` | `0.7` | LLM temperature |

## Architecture

```
┌─────────────────────────────────────────────────┐
│              VS Code Extension                   │
│  ┌──────────┐  ┌──────────┐  ┌───────────────┐  │
│  │ Chat     │  │ Config   │  │ Status Bar    │  │
│  │ WebView  │  │ WebView  │  │ Button        │  │
│  └────┬─────┘  └────┬─────┘  └───────┬───────┘  │
│       │              │                │          │
│  ┌────▼──────────────▼────────────────▼──────┐  │
│  │           Extension Host                   │  │
│  │  ┌──────────┐  ┌──────────┐               │  │
│  │  │ Chat     │  │ Config   │               │  │
│  │  │ Service  │  │ Service  │               │  │
│  │  └────┬─────┘  └────┬─────┘               │  │
│  │       │              │                     │  │
│  │  ┌────▼──────────────▼──────┐              │  │
│  │  │      MCP Client          │              │  │
│  │  │  (JSON-RPC 2.0 / stdio)  │              │  │
│  │  └────────────┬─────────────┘              │  │
│  └───────────────┼────────────────────────────┘  │
│                  │ stdin/stdout                  │
│  ┌───────────────▼────────────────────────────┐  │
│  │         Spire Rust Core (subprocess)       │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  │  │
│  │  │ MCP      │  │ Agent    │  │ Memory   │  │  │
│  │  │ Server   │  │ System   │  │ Graph    │  │  │
│  │  └──────────┘  └──────────┘  └──────────┘  │  │
│  └─────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

## Development

### Prerequisites

- [Node.js](https://nodejs.org/) >= 18
- [VS Code](https://code.visualstudio.com/) >= 1.85.0
- Rust toolchain (for building the core binary)

### Setup

```bash
# Install dependencies
cd spire-vscode
npm install

# Compile TypeScript
npx tsc --noEmit

# Open in VS Code and press F5 to launch a new Extension Development Host
code .
```

### Project Structure

```
spire-vscode/
├── package.json              # Extension manifest
├── tsconfig.json              # TypeScript configuration
├── .vscode/launch.json        # VS Code debug launch configuration
├── src/
│   ├── extension.ts           # Extension entry point (activate/deactivate)
│   ├── mcp/
│   │   ├── types.ts           # MCP protocol type definitions
│   │   └── client.ts          # MCP stdio client with auto-reconnect
│   ├── services/
│   │   ├── chat.ts            # Chat session management & streaming
│   │   └── config.ts          # Configuration management & agent runner
│   └── webviews/
│       ├── chat.ts            # Chat panel WebView UI
│       └── config.ts          # Configuration editor WebView UI
```

## License

This project is licensed under the terms found in the [LICENSE](../LICENSE) file at the root of the repository.
