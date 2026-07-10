# Spire Test Suite Reference

> **Status:** Complete  
> **Last Updated:** 2026-07-09  
> **See also:** `doc/actors-and-messages.md` (architecture), `doc/extension-core-interface.md` (protocol)

## Overview

The Spire project maintains **2 test files** in the Rust core, plus **3 test files** in the VS Code extension:

| Layer | Language | Count | Files |
|-------|----------|-------|-------|
| **Rust Core** (`spire-core/tests/`) | Rust (cargo test) | 2 | `actor_tests.rs`, `integration_tests.rs` |
| **VS Code Extension** (`spire-extension/test/`) | TypeScript (mocha) | 3 | `communication.test.mjs`, `handler-integration.test.mjs`, `mock-env-server.mjs` |

---

## Rust Test Files

### 1. `spire-core/tests/actor_tests.rs` — Unit Tests

**File:** `spire-core/tests/actor_tests.rs`  
**Lines:** 547  
**Test count:** ~20

Tests individual actors in isolation by importing `spire_core` as a library, creating an `ActorSystem`, spawning actors directly, and sending messages via their `mpsc::Sender` channels.

| # | Test Name | What It Verifies |
|---|-----------|------------------|
| **ChatActor** | | |
| 1 | `test_chat_get_active_returns_default` | Default dialog exists with id "default" and title "New Chat" |
| 2 | `test_chat_create_new_dialog` | New dialog has unique ID, correct title, empty messages |
| 3 | `test_chat_send_message` | Message is stored with correct content, role, and non-zero timestamp |
| 4 | `test_chat_get_history` | History returns messages in order |
| 5 | `test_chat_list_dialogs` | List returns all dialogs with correct summaries |
| 6 | `test_chat_delete_dialog` | Deleted dialog is removed from list |
| 7 | `test_chat_delete_nonexistent` | Deleting nonexistent dialog returns false |
| 8 | `test_chat_send_message_to_nonexistent_dialog` | Returns error for unknown dialog |
| 9 | `test_chat_multiple_dialogs_independent` | Messages in different dialogs don't interfere |
| **ToolsActor** | | |
| 10 | `test_tools_list_returns_initial_tools` | Initial tool list is not empty |
| 11 | `test_tools_list_contains_expected_tools` | Known tools (echo, read_file, etc.) are present |
| 12 | `test_tools_call_echo` | Echo tool returns the input message |
| 13 | `test_tools_call_unknown_tool` | Unknown tool returns ToolNotFound error |
| **McpClientActor** | | |
| 14 | `test_mcp_client_load_config` | LoadConfig returns a path or None |
| 15 | `test_mcp_client_connected_servers_empty` | Initially no connected servers |
| **LlmActor** | | |
| 16 | `test_llm_complete_echoes_prompt` | LLM stub echoes back the prompt |
| 17 | `test_llm_stream_returns_receiver` | Stream returns a valid Receiver |
| **ProgressActor** | | |
| 18 | `test_progress_publish_and_subscribe` | Subscriber receives published updates |
| 19 | `test_progress_multiple_subscribers` | All subscribers receive all updates |
| **SystemActor** | | |
| 20 | `test_system_health_returns_status` | Health check returns a valid status object |

**Key imports:** `spire_core::actors::*`, `spire_core::framework::ActorSystem`

---

### 2. `spire-core/tests/integration_tests.rs` — Integration Tests

**File:** `spire-core/tests/integration_tests.rs`  
**Lines:** 369  
**Test count:** ~10

Black-box tests that build the `spire-core` binary, spawn it as a child process, send JSON-RPC 2.0 requests via stdin, and read responses from stdout.

| # | Test Name | What It Verifies |
|---|-----------|------------------|
| 1 | `test_health_request` | `{"jsonrpc":"2.0","method":"health","id":1}` returns valid response |
| 2 | `test_list_tools_request` | `tools/list` returns a non-empty tool array |
| 3 | `test_call_tool_echo` | `tools/call` with echo tool returns the input message |
| 4 | `test_call_tool_unknown` | Unknown tool returns error response |
| 5 | `test_invalid_json` | Malformed JSON returns parse error |
| 6 | `test_unknown_method` | Unknown method returns method not found error |
| 7 | `test_concurrent_requests` | Multiple concurrent requests all receive responses |
| 8 | `test_shutdown` | Shutdown request terminates the process gracefully |
| 9 | `test_reconnect` | Process can be restarted after shutdown |
| 10 | `test_long_running_session` | Process stays alive across multiple sequential requests |

**Key imports:** `std::process::{Command, Stdio}`, `std::io::{BufRead, BufReader, Write}`

---

## TypeScript Test Files

### 3. `spire-extension/test/communication.test.mjs`

**File:** `spire-extension/test/communication.test.mjs`  
**Lines:** ~200  
**Test count:** ~8

Tests the JSON-RPC communication protocol between the extension and the Rust core subprocess.

| # | Test Name | What It Verifies |
|---|-----------|------------------|
| 1 | `test_send_request` | Sends a JSON-RPC request and receives a response |
| 2 | `test_receive_notification` | Receives a notification from the core |
| 3 | `test_request_timeout` | Request times out if no response |
| 4 | `test_invalid_json_response` | Malformed response is handled gracefully |
| 5 | `test_concurrent_requests` | Multiple concurrent requests all resolve |
| 6 | `test_reconnect_after_disconnect` | Client reconnects after subprocess restart |
| 7 | `test_bidirectional_messaging` | Both sides can send requests |
| 8 | `test_message_ordering` | Messages are processed in order |

---

### 4. `spire-extension/test/handler-integration.test.mjs`

**File:** `spire-extension/test/handler-integration.test.mjs`  
**Lines:** ~250  
**Test count:** ~12

Tests the VS Code extension's request handlers against a mock environment server.

| # | Test Name | What It Verifies |
|---|-----------|------------------|
| **Workspace** | | |
| 1 | `test_workspace_get_workspace_folders` | Returns workspace folder paths |
| 2 | `test_workspace_get_configuration` | Returns VS Code configuration |
| **Document** | | |
| 3 | `test_document_open_document` | Opens a document and returns its content |
| 4 | `test_document_get_text` | Returns text content of an open document |
| **Editor** | | |
| 5 | `test_editor_get_active_editor` | Returns active editor info |
| 6 | `test_editor_edit_document` | Applies edits to a document |
| **Diagnostics** | | |
| 7 | `test_diagnostics_get_diagnostics` | Returns diagnostics for a file |
| **Git** | | |
| 8 | `test_git_get_status` | Returns git status |
| **Terminal** | | |
| 9 | `test_terminal_create_and_write` | Creates a terminal and writes to it |
| **Symbols** | | |
| 10 | `test_symbols_get_document_symbols` | Returns document symbols |
| **Chat** | | |
| 11 | `test_chat_send_message` | Sends a chat message and receives response |
| **Error Handling** | | |
| 12 | `test_unknown_method` | Unknown method returns error response |

---

### 5. `spire-extension/test/mock-env-server.mjs`

**File:** `spire-extension/test/mock-env-server.mjs`  
**Lines:** ~150

A mock VS Code environment server used by the handler integration tests. Simulates VS Code API responses for testing purposes.

---

## Running Tests

### Rust Tests

```bash
# Run all Rust tests (from spire-core/)
cd spire-core && cargo test

# Run with output (no capture)
cargo test -- --nocapture

# Run a specific test
cargo test test_chat_get_active_returns_default -- --nocapture

# Run by module
cargo test actor_tests       # Unit tests
cargo test integration       # Integration tests
```

### TypeScript Tests

```bash
# From spire-extension/
cd spire-extension

# Run all TS tests
npm test

# Run specific test file
node test/communication.test.mjs
```

---

## Test Coverage Summary

| Area | Tests | Files | Description |
|------|-------|-------|-------------|
| **ChatActor** | 9 | Rust | Dialog CRUD, message history, isolation |
| **ToolsActor** | 4 | Rust | List, call, error handling |
| **McpClientActor** | 2 | Rust | Config loading, server listing |
| **LlmActor** | 2 | Rust | Complete, stream |
| **ProgressActor** | 2 | Rust | Publish/subscribe, multiple subscribers |
| **SystemActor** | 1 | Rust | Health check |
| **Integration (JSON-RPC)** | 10 | Rust | Binary lifecycle, request/response, concurrency |
| **Communication** | 8 | TS | JSON-RPC protocol, timeouts, reconnection |
| **Handler Integration** | 12 | TS | VS Code API handlers against mock server |
| **Total** | **~50** | **5 files** | Rust: ~30, TS: ~20 |

---

## Coverage Gaps (Not Yet Tested)

The following areas are **not yet covered** by automated tests:

1. **MCP client connections** — Actual WebSocket/stdio connections to external MCP servers untested
2. **LLM provider calls** — Only stub tested; actual API calls untested
3. **VSCode tool callbacks** — Tool definitions tested, but actual VS Code callback path untested
4. **WebView sidebar** — UI components not tested
5. **Extension activation/deactivation** — Requires VS Code API testing infrastructure
