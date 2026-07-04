# Changelog

All notable changes to Spire Rust will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **License**: Changed from MIT to GNU GPLv3 (GPL-3.0-or-later)
  - Replaced `LICENSE` file with full GPLv3 text
  - Added GPLv3 SPDX and copyright headers to all source files
  - Updated README files with new license references

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
