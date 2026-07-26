# Test Suite Reference

> **Last updated:** 2026-07-26

This document catalogs all test files in the project — Rust unit tests, actor tests, and TypeScript extension tests.

---

## Rust Tests (`rust/spire-core/tests/`)

| Test File | Tests | Purpose |
|-----------|-------|---------|
| `actor_tests.rs` | ~8 | Framework-level actor tests — spawning, message passing, shutdown |
| `integration_tests.rs` | ~10 | End-to-end integration tests across multiple actors |
| `tool_orchestrator_tests.rs` | 20 | Unit and integration tests for ToolOrchestrator — step execution, template resolution, tool chains |
| `plan_orchestrator_tests.rs` | 14 | **New!** Tests for PlanOrchestrator — plan/step CRUD in graph, status queries, reject flow, dependency ordering |

### Running Tests

```bash
# All tests
cd rust/spire-core && cargo test

# Single test file
cargo test -p spire-core --test plan_orchestrator_tests

# Single test
cargo test -p spire-core --test plan_orchestrator_tests test_plan_node_store_and_retrieve
```

---

## Rust Unit Tests (`rust/spire-core/src/`)

Embedded in source files. Key modules with `#[cfg(test)]` blocks:

| File | What's Tested |
|------|---------------|
| `actors/tool_orchestrator.rs` | `StepContext` template resolution (`$error.file`, `$step.1.output`) |
| `actors/intent_router.rs` | Pattern matching logic |
| `actors/memory_graph.rs` | Graph CRUD operations |
| `analyser/build_parsers/*.rs` | Individual build system parsers (Cargo, Node, CMake, etc.) |

---

## MCP Server Tests (`rust/mcp/`)

Each MCP server has its own Cargo.toml with tests:

```bash
cd rust && cargo test --workspace
```

| MCP Server | Test Focus |
|------------|-----------|
| `mcp-git` | Git operations (clone, status, diff, log) |
| `mcp-process` | Process spawning and management |
| `mcp-search` | Code search (ripgrep-based) |
| `mcp-terminal` | Terminal session management |
| `mcp-filesystem` | File read/write operations |
| `mcp-cargo` | Cargo build and analyze operations |
| `mcp-node` | Node.js build and analyze operations |

---

## TypeScript Extension Tests (`ts/spire-extension/test/`)

| Test File | Purpose |
|-----------|---------|
| `communication.test.mjs` | Tests JSON-RPC communication between extension and core |
| `handler-integration.test.mjs` | Integration tests for request/response handlers |
| `mock-env-server.mjs` | Mock server for testing |
| `test-harness.mjs` | Test harness utilities |

```bash
cd ts/spire-extension && npm test
```

---

## Test Summary

| Layer | Pass | Source |
|-------|------|--------|
| PlanOrchestrator | 14/14 | `tests/plan_orchestrator_tests.rs` |
| ToolOrchestrator | 22/28 | `tests/tool_orchestrator_tests.rs` (6 require GQL init) |
| Actor framework | 8/8 | `tests/actor_tests.rs` |
| Integration | ~10 | `tests/integration_tests.rs` |
| MCP Servers | ~50 | `rust/mcp/*/` |
| TypeScript extension | ~20 | `ts/spire-extension/test/` |

Total: ~130 tests across the project.