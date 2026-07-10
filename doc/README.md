# Spire Rust — Documentation

Reference documentation for the Spire Rust project.

---

## Contents

| Document | Description |
|----------|-------------|
| [`extension-core-interface.md`](extension-core-interface.md) | Complete reference for the JSON-RPC 2.0 interface between the VS Code extension and the Rust core — transport, protocol, tool catalog, notifications, lifecycle, error handling, and sequence diagrams |
| [`messages-and-types.md`](messages-and-types.md) | Complete reference for the actor system — message enums, reply channels, and all shared data structures |
| [`actors-and-messages.md`](actors-and-messages.md) | Catalog of every actor in the system, their message enum variants, and how they connect |
| [`graph-schema.md`](graph-schema.md) | Complete graph schema reference — node types, relationship types, constraints, and physical storage mapping |
| [`json-rpc-protocol.md`](json-rpc-protocol.md) | JSON-RPC 2.0 message reference for extension–core communication |
| [`agent-implementation-instructions.md`](agent-implementation-instructions.md) | Agent implementation guidelines and patterns |
| [`agent-infrastructure.md`](agent-infrastructure.md) | Agent infrastructure overview and architecture |
| [`vscode-environment-model.md`](vscode-environment-model.md) | VS Code environment model reference |
| [`packaging-structure.md`](packaging-structure.md) | Binary packaging and staging guide |
| [`test-suite-reference.md`](test-suite-reference.md) | Reference for all test files — Rust unit tests, JSON-RPC integration tests, and TypeScript extension tests |

---

## `messages-and-types.md`

This is the primary reference document for the `spire-core/` actor system. It covers:

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
- [CONTRIBUTING.md](../CONTRIBUTING.md) — Contribution guidelines
