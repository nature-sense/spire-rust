# Spire Rust — Documentation

Reference documentation for the Spire Rust project.

---

## Contents

| Document | Description |
|----------|-------------|
| [`extension-core-interface.md`](extension-core-interface.md) | Complete reference for the JSON-RPC 2.0 interface between the VS Code extension and the Rust core — transport, protocol, tool catalog, notifications, lifecycle, error handling, and sequence diagrams |
| [`messages-and-types.md`](messages-and-types.md) | Complete reference for the actor system — message enums, reply channels, and all shared data structures |

---

## `messages-and-types.md`

This is the primary reference document for the `core/` actor system. It covers:

- **Actor System Overview** — The four actors (`CoordinatorActor`, `MemoryGraphActor`, `ProgressActor`, `LlmActor`) and how they communicate
- **Message Definitions** — All message variants with their reply types and descriptions:
  - `CoordinatorMessage` (3 variants)
  - `MemoryGraphMessage` (14 variants)
  - `ProgressMessage` (2 variants)
  - `LlmMessage` (2 variants)
- **Data Model Types** — Full struct definitions for:
  - `GraphNode`, `NodeType`, `NodeInput`, `NodeUpdate`, `NodeFilter`
  - `GraphEdge`, `RelationshipType`, `RelationshipInput`
  - `TraversalOptions`, `TraversalDirection`, `TraversalResult`, `TraversalPath`
  - `ProjectSnapshot`, `ProjectStats`
  - `SearchOptions`, `ContextSearchResult`, `ScoredNode`, `RetrievalSource`
  - `MemoryMetadata`, `MemoryEntry`
  - `GraphQuery`, `GraphQueryType`, `GraphResult`
  - `CodeAnalysis`, `CodeAnalysisRequest`, `ComplexityScore`, `SymbolInfo`, `SearchResult`
  - `Embedding`, `Embedder` trait
- **MCP Layer** — Server handler, tool definitions, and external client
- **Progress Types** — `ProgressUpdate` and `ProgressStatus`
- **Actor Wiring Diagram** — How the actors are spawned and connected in `main.rs`

---

## Related

- [Root README](../README.md) — Project overview and quick start
- [Core README](../core/README.md) — Rust MCP server documentation
- [Spire VS Code README](../spire-vscode/README.md) — VS Code extension documentation
- [CONTRIBUTING.md](../CONTRIBUTING.md) — Contribution guidelines
- [CHANGELOG.md](../CHANGELOG.md) — Release history

