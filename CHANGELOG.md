# Changelog

All notable changes to Spire Rust will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Snapshot persistence for GraphDb** (`spire-core/src/graph/mod.rs`)
  - `GraphDb::recover()` — recover graph from snapshot + WAL on disk
  - `GraphDb::write_snapshot()` — write point-in-time snapshot with zstd compression
  - `GraphDb::latest_snapshot_sequence()` — query latest snapshot sequence number
  - `GraphDb::snapshot_path()` — canonical snapshot file path helper
  - 6 new tests covering snapshot write, recovery, empty dir, multiple snapshots, and path format

- **Standalone MCP servers** (`mcp/`)
  - `mcp-git` — Git operations MCP server (status, log, diff, blame)
  - `mcp-process` — Process management MCP server (spawn, list, kill)
  - `mcp-search` — Code search MCP server (ripgrep-based regex search)

- **Documentation** (`doc/`)
  - `graph-schema.md` — Complete graph schema reference (node types, relationship types, constraints, physical storage mapping)
  - `json-rpc-protocol.md` — JSON-RPC 2.0 message reference for extension–core communication
  - `actors-and-messages.md` — Actor catalog with message variants and wiring
  - `agent-implementation-instructions.md` — Agent implementation guidelines
  - `agent-infrastructure.md` — Agent infrastructure overview
  - `vscode-environment-model.md` — VS Code environment model reference
  - `packaging-structure.md` — Binary packaging and staging guide
  - `test-suite-reference.md` — Test suite reference

### Changed

- **Project restructure**: Renamed `core/` → `spire-core/` and `spire-vscode/` → `spire-extension/` for consistent naming
- **License**: Changed from MIT to GNU GPLv3 (GPL-3.0-or-later)
  - Replaced `LICENSE` file with full GPLv3 text
  - Added GPLv3 SPDX and copyright headers to all source files
  - Updated README files with new license references
- **Root `Cargo.toml`**: Added workspace manifest for `spire-core/` and MCP servers
- **`pnpm-workspace.yaml`**: Updated to reference `spire-extension/` instead of `spire-vscode/`
- **`.gitignore`**: Updated for new project structure

## [0.1.0] - 2025-01-07

### Added

- **Rust MCP Server** (`core/`)
  - MCP protocol layer with `rust-mcp-sdk` — handles `list_tools` and `call_tool` requests
  - Four MCP tools: `explain_code`, `search_codebase`, `analyze_dependencies`, `get_code_metrics`
  - Actor system using `tonari-actor` with four actors:
    - `CoordinatorActor` — workflow orchestrator
    - `MemoryGraphActor` — sole data store for graph nodes, edges, and embeddings
    - `ProgressActor` — progress broadcasting via `tokio::sync::broadcast`
    - `LlmActor` — LLM gateway client (stub)
  - Text embedding via Candle (`all-MiniLM-L6-v2`, 384-dimensional vectors)
  - Graph database wrapper around SeleneDB with WAL persistence
  - External MCP client manager for connecting to third-party MCP servers

- **VS Code Extension** (`extension/`)
  - Thin TypeScript UI shell that spawns the Rust binary as a child process
  - JSON-RPC MCP client over stdio with 30-second request timeout
  - Webview-based chat panel with Markdown rendering and progress bars
  - Status bar indicator (Ready/Working/Error states)
  - Four commands with keybindings: Explain Code, Open Chat, Search Codebase, Analyze Code

- **Documentation** (`doc/`)
  - Complete actor message and data type reference (`messages-and-types.md`)
  - README files for root, core, extension, and doc directories

### Notes

- This is an initial release with stub implementations for LLM calls and code analysis
- The actor system is wired up but the coordinator is not yet spawned in `main.rs`
- Embedding model (~85 MB) downloads on first run from Hugging Face Hub
- Metal GPU acceleration is opt-in via `SPIRE_USE_METAL=1` (CPU by default)
