// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! Black-box integration tests for spire-core.
//!
//! These tests build the `spire-core` binary, spawn it as a child process,
//! send JSON-RPC 2.0 requests via stdin, read responses from stdout,
//! and assert on the results.
//!
//! Each test:
//! 1. Builds the binary (or uses a pre-built one)
//! 2. Spawns the process with stdin/stdout pipes
//! 3. Writes a JSON-RPC request line to stdin
//! 4. Reads a JSON line from stdout
//! 5. Asserts on the response

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// Helper to build the spire-core binary and return its path.
fn build_binary() -> std::path::PathBuf {
    // Use CARGO_MANIFEST_DIR to find the project root
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR")
        .expect("CARGO_MANIFEST_DIR must be set (run via cargo test)");

    // Build the binary
    let status = Command::new("cargo")
        .args(["build", "--bin", "spire-core", "--manifest-path"])
        .arg(format!("{}/Cargo.toml", manifest_dir))
        .status()
        .expect("Failed to build spire-core binary");

    assert!(status.success(), "cargo build failed");

    // Determine the binary path
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from(&manifest_dir).join("target"));

    let profile = if cfg!(debug_assertions) { "debug" } else { "release" };
    target_dir.join(profile).join("spire-core")
}

/// A test harness that manages a spire-core subprocess.
struct CoreProcess {
    child: Child,
    stdin_writer: std::io::BufWriter<std::process::ChildStdin>,
    stdout_reader: BufReader<std::process::ChildStdout>,
}

impl CoreProcess {
    /// Spawn a new spire-core process.
    fn spawn() -> Self {
        let binary_path = build_binary();

        let mut child = Command::new(&binary_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null()) // Suppress stderr in tests
            .spawn()
            .expect("Failed to spawn spire-core process");

        let stdin = child.stdin.take().expect("Failed to capture stdin");
        let stdout = child.stdout.take().expect("Failed to capture stdout");

        // Give the process a moment to start up
        std::thread::sleep(Duration::from_millis(500));

        Self {
            child,
            stdin_writer: std::io::BufWriter::new(stdin),
            stdout_reader: BufReader::new(stdout),
        }
    }

    /// Send a JSON-RPC request and read the response.
    fn request(&mut self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        // Write the request to stdin
        let line = serde_json::to_string(&request).unwrap();
        writeln!(self.stdin_writer, "{}", line).unwrap();
        self.stdin_writer.flush().unwrap();

        // Read the response from stdout
        let mut response_line = String::new();
        self.stdout_reader.read_line(&mut response_line).unwrap();
        let response_line = response_line.trim().to_string();

        // Parse the response
        let response: serde_json::Value = serde_json::from_str(&response_line)
            .unwrap_or_else(|e| panic!("Failed to parse response JSON: {} — raw: {}", e, response_line));

        response
    }

    /// Send a raw line to stdin (for malformed JSON tests).
    fn send_raw(&mut self, line: &str) -> String {
        writeln!(self.stdin_writer, "{}", line).unwrap();
        self.stdin_writer.flush().unwrap();

        let mut response_line = String::new();
        self.stdout_reader.read_line(&mut response_line).unwrap();
        response_line.trim().to_string()
    }

    /// Send a JSON-RPC request with a specific ID.
    fn request_with_id(
        &mut self,
        id: u64,
        method: &str,
        params: serde_json::Value,
    ) -> serde_json::Value {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });

        let line = serde_json::to_string(&request).unwrap();
        writeln!(self.stdin_writer, "{}", line).unwrap();
        self.stdin_writer.flush().unwrap();

        let mut response_line = String::new();
        self.stdout_reader.read_line(&mut response_line).unwrap();
        let response_line = response_line.trim().to_string();

        serde_json::from_str(&response_line)
            .unwrap_or_else(|e| panic!("Failed to parse response JSON: {} — raw: {}", e, response_line))
    }
}

impl Drop for CoreProcess {
    fn drop(&mut self) {
        // Close stdin to signal EOF
        let _ = self.stdin_writer.flush();
        // Kill the process
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[test]
fn test_ping() {
    let mut core = CoreProcess::spawn();
    let response = core.request("ping", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"], serde_json::json!({"pong": true}));
}

#[test]
fn test_system_status() {
    let mut core = CoreProcess::spawn();
    let response = core.request("system/status", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["status"], "running");
    assert_eq!(response["result"]["version"], "0.1.0");
    assert!(response["result"]["uptime_seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(response["result"]["actors"]["chat"], true);
    assert_eq!(response["result"]["actors"]["system"], true);
}

#[test]
fn test_chat_get_active() {
    let mut core = CoreProcess::spawn();
    let response = core.request("chat/getActive", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["id"], "default");
    assert_eq!(response["result"]["title"], "New Chat");
    assert!(response["result"]["messages"].as_array().unwrap().is_empty());
}

#[test]
fn test_chat_append_and_get_history() {
    let mut core = CoreProcess::spawn();

    // Append a message
    let append_response = core.request("chat/append", serde_json::json!({
        "chatId": "default",
        "content": "Hello from black-box test",
        "options": {"role": "user"}
    }));

    assert_eq!(append_response["jsonrpc"], "2.0");
    assert_eq!(append_response["id"], 1);
    assert_eq!(append_response["result"]["content"], "Hello from black-box test");
    assert_eq!(append_response["result"]["role"], "user");

    // Get history
    let history_response = core.request("chat/getHistory", serde_json::json!({}));

    assert_eq!(history_response["jsonrpc"], "2.0");
    assert_eq!(history_response["id"], 1);
    let dialogs = history_response["result"].as_array().unwrap();
    assert_eq!(dialogs.len(), 1);
    assert_eq!(dialogs[0]["messages"][0]["content"], "Hello from black-box test");
}

#[test]
fn test_chat_clear() {
    let mut core = CoreProcess::spawn();

    // Append a message
    core.request("chat/append", serde_json::json!({
        "chatId": "default",
        "content": "to_clear",
        "options": {"role": "user"}
    }));

    // Clear
    let clear_response = core.request("chat/clear", serde_json::json!({
        "chatId": "default"
    }));
    assert_eq!(clear_response["result"]["success"], true);

    // Verify empty
    let active_response = core.request("chat/getActive", serde_json::json!({}));
    assert!(active_response["result"]["messages"].as_array().unwrap().is_empty());
}

#[test]
fn test_chat_set_title() {
    let mut core = CoreProcess::spawn();

    let response = core.request("chat/setTitle", serde_json::json!({
        "chatId": "default",
        "title": "Integration Test Chat"
    }));
    assert_eq!(response["result"]["success"], true);

    // Verify
    let active_response = core.request("chat/getActive", serde_json::json!({}));
    assert_eq!(active_response["result"]["title"], "Integration Test Chat");
}

#[test]
fn test_tools_list_empty() {
    let mut core = CoreProcess::spawn();
    let response = core.request("tools/list", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    // ToolsActor pre-registers VS Code extension tools at startup
    assert!(!response["result"].as_array().unwrap().is_empty());
    assert!(response["result"].as_array().unwrap().iter().any(|t| t["name"] == "workspace/getFolders"));
}

#[test]
fn test_mcp_servers_loaded() {
    let mut core = CoreProcess::spawn();
    let response = core.request("mcp/servers", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    // MCP config is loaded at startup from ~/.spire/mcp-config.json,
    // so the servers list should contain at least the filesystem server
    eprintln!("DEBUG mcp/servers response: {}", response);
    let servers = response["result"].as_array().unwrap();
    assert!(!servers.is_empty(), "Expected MCP servers to be loaded from config. Response: {}", response);
    // Now the response is a list of McpServerDetail objects with a "name" field
    assert!(servers.iter().any(|s| s.get("name").and_then(|v| v.as_str()) == Some("filesystem")),
        "Expected 'filesystem' server to be in the list: {:?}", servers);
}


#[test]
fn test_unknown_method_returns_error() {
    let mut core = CoreProcess::spawn();
    let response = core.request("unknown/method", serde_json::json!({}));

    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    // The coordinator returns the error as a result value
    assert!(response["result"].get("error").is_some());
    assert!(response["result"]["error"].as_str().unwrap().contains("unknown/method"));
}

#[test]
fn test_multiple_sequential_requests() {
    let mut core = CoreProcess::spawn();

    // Request 1: ping
    let r1 = core.request_with_id(1, "ping", serde_json::json!({}));
    assert_eq!(r1["id"], 1);
    assert_eq!(r1["result"]["pong"], true);

    // Request 2: system status
    let r2 = core.request_with_id(2, "system/status", serde_json::json!({}));
    assert_eq!(r2["id"], 2);
    assert_eq!(r2["result"]["status"], "running");

    // Request 3: chat append
    let r3 = core.request_with_id(3, "chat/append", serde_json::json!({
        "chatId": "default",
        "content": "multi-request",
        "options": {"role": "user"}
    }));
    assert_eq!(r3["id"], 3);
    assert_eq!(r3["result"]["content"], "multi-request");

    // Request 4: chat getHistory (should have the message from request 3)
    let r4 = core.request_with_id(4, "chat/getHistory", serde_json::json!({}));
    assert_eq!(r4["id"], 4);
    assert_eq!(r4["result"][0]["messages"][0]["content"], "multi-request");
}

#[test]
fn test_chat_append_without_options_defaults_to_assistant() {
    let mut core = CoreProcess::spawn();

    let response = core.request("chat/append", serde_json::json!({
        "chatId": "default",
        "content": "default role message"
    }));

    assert_eq!(response["result"]["role"], "assistant");
    assert_eq!(response["result"]["content"], "default role message");
}

#[test]
fn test_system_config_get_unknown() {
    let mut core = CoreProcess::spawn();

    let response = core.request("system/config/get", serde_json::json!({
        "key": "nonexistent"
    }));

    assert_eq!(response["result"]["value"], serde_json::Value::Null);
}

#[test]
fn test_mcp_connect_unknown_server() {
    let mut core = CoreProcess::spawn();

    let response = core.request("mcp/connect", serde_json::json!({
        "serverName": "nonexistent_server"
    }));

    assert!(response.get("error").is_some() || response.get("result").is_some());
    // The MCP client actor will return an error since the server doesn't exist
    // in its config. This test just verifies the routing works without crashing.
}

#[test]
fn test_mcp_get_tools_unknown_server() {
    let mut core = CoreProcess::spawn();

    let response = core.request("mcp/getTools", serde_json::json!({
        "serverName": "nonexistent"
    }));

    // Should return an empty array for unknown server
    assert_eq!(response["result"], serde_json::json!([]));
}
