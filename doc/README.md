# Spire Rust — Documentation

Reference documentation for the Spire Rust project.

---

## Contents

| Document | Description |
|----------|-------------|
| [`actors-and-messages.md`](actors-and-messages.md) | **New!** Complete catalog of all 20+ actors, their message enum variants, and wiring diagram |
| [`messages-and-types.md`](messages-and-types.md) | **New!** Quick reference for all shared data types (graph nodes, plans, build results, fix plans) |
| [`multi-step-flows.md`](multi-step-flows.md) | **Updated!** Execution flow diagrams for the build-fix loop, plan mode, and error analysis |
| [`extension-core-interface.md`](extension-core-interface.md) | JSON-RPC 2.0 interface spec between the VS Code extension and the Rust core |
| [`graph-schema.md`](graph-schema.md) | Graph schema reference — node types, relationship types, constraints, and storage mapping |
| [`json-rpc-protocol.md`](json-rpc-protocol.md) | JSON-RPC 2.0 message reference for extension–core communication |
| [`packaging-structure.md`](packaging-structure.md) | Binary packaging and staging guide |
| [`test-suite-reference.md`](test-suite-reference.md) | **Updated!** Test file catalog — Rust unit tests, actor tests, and integration tests |
| [`actor.md`](actor.md) | Actor pattern design guidelines for implementing new actors |

### Deleted (Superseded)

| Document | Replaced By |
|----------|-------------|
| `agent-infrastructure.md` | `actors-and-messages.md` + `graph-schema.md` |
| `agent-implementation-instructions.md` | `actors-and-messages.md` + root README |
| `vscode-environment-model.md` | `extension-core-interface.md` |

---

## Related

- [Root README](../README.md) — Project overview, architecture, and quick start
- [CONTRIBUTING.md](../CONTRIBUTING.md) — Contribution guidelines