// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! JSON-RPC 2.0 transport over stdin/stdout.
//!
//! This module provides bidirectional JSON-RPC communication:
//! - Reads newline-delimited JSON from stdin
//! - Writes newline-delimited JSON responses to stdout
//! - Supports sending requests TO the extension (for VS Code API calls)
//! - Supports sending notifications (events) to the extension
//!
//! Protocol: https://www.jsonrpc.org/specification
//!
//! Messages from extension → core (stdin):
//!   {"jsonrpc":"2.0","id":1,"method":"chat/getActive","params":{}}
//!
//! Responses from core → extension (stdout):
//!   {"jsonrpc":"2.0","id":1,"result":{...}}
//!
//! Requests from core → extension (stdout):
//!   {"jsonrpc":"2.0","id":100,"method":"workspace/getFolders","params":{}}
//!
//! Responses from extension → core (stdin):
//!   {"jsonrpc":"2.0","id":100,"result":[...]}
//!
//! Notifications (no id):
//!   {"jsonrpc":"2.0","method":"event/chat/message","params":{...}}

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, error, info, warn};

/// A pending outgoing request (core → extension) waiting for a response.
struct PendingRequest {
    response_tx: oneshot::Sender<Result<serde_json::Value, String>>,
    #[allow(dead_code)]
    method: String,
}

/// Handler for incoming JSON-RPC requests from the extension.
pub type RequestHandler = Arc<dyn Fn(serde_json::Value) -> serde_json::Value + Send + Sync>;

/// Bidirectional JSON-RPC 2.0 transport over stdin/stdout.
pub struct Transport {
    /// Sender channel to push lines from the stdin reader task.
    _line_tx: mpsc::UnboundedSender<String>,
    /// Join handle for the stdin reader task.
    _reader_handle: Option<tokio::task::JoinHandle<()>>,
    /// Pending outgoing requests awaiting responses.
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    /// Next request ID for outgoing requests.
    next_id: Arc<Mutex<u64>>,
    /// Handler for incoming requests from the extension.
    request_handler: Arc<Mutex<Option<RequestHandler>>>,
}

impl Transport {
    /// Create a new transport.
    ///
    /// The stdin reader is NOT started until `start()` is called.
    /// This allows the caller to set up the request handler first,
    /// avoiding race conditions where requests arrive before the
    /// handler is registered.
    pub fn new() -> Self {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));
        let request_handler: Arc<Mutex<Option<RequestHandler>>> = Arc::new(Mutex::new(None));

        Self {
            _line_tx: mpsc::unbounded_channel::<String>().0,
            _reader_handle: None,
            pending,
            next_id,
            request_handler,
        }
    }

    /// Set the handler for incoming requests from the extension.
    pub async fn set_request_handler(&self, handler: RequestHandler) {
        let mut h = self.request_handler.lock().await;
        *h = Some(handler);
    }

    /// Start the stdin reader and line processor tasks.
    ///
    /// Must be called AFTER `set_request_handler()` to avoid race conditions.
    pub fn start(&mut self) {
        let (line_tx, mut line_rx) = mpsc::unbounded_channel::<String>();
        let pending = self.pending.clone();
        let handler = self.request_handler.clone();

        // Spawn a task to read lines from stdin
        let reader_handle = tokio::task::spawn_blocking(move || {
            let stdin = io::stdin();
            let reader = stdin.lock();
            for line in reader.lines() {
                match line {
                    Ok(text) => {
                        let trimmed = text.trim().to_string();
                        if !trimmed.is_empty() {
                            if let Err(e) = line_tx.send(trimmed) {
                                error!("Transport: failed to send line to processor: {}", e);
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Transport: error reading stdin: {}", e);
                        break;
                    }
                }
            }
            info!("Transport: stdin reader task finished");
        });

        // Spawn a task to process incoming lines
        tokio::spawn(async move {
            while let Some(line) = line_rx.recv().await {
                Self::process_line(&line, &pending, &handler).await;
            }
        });

        self._reader_handle = Some(reader_handle);
    }

    /// Send a JSON-RPC response to the extension (stdout).
    pub fn send_response(id: u64, result: &serde_json::Value) {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        Self::write_json(&response);
    }

    /// Send a JSON-RPC error response to the extension.
    pub fn send_error(id: u64, code: i64, message: &str) {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            },
        });
        Self::write_json(&response);
    }

    /// Send a notification (event) to the extension.
    pub fn send_notification(method: &str, params: &serde_json::Value) {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        Self::write_json(&notification);
    }

    /// Send a request to the extension and wait for a response.
    ///
    /// This is used when the core needs to call VS Code API methods
    /// (e.g., workspace/getFolders, editor/getActive).
    pub async fn call_extension(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut id_lock = self.next_id.lock().await;
        let id = *id_lock;
        *id_lock += 1;
        drop(id_lock);

        let (response_tx, response_rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, PendingRequest {
                response_tx,
                method: method.to_string(),
            });
        }

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        Self::write_json(&request);

        // Wait for the response with a timeout
        match tokio::time::timeout(std::time::Duration::from_secs(30), response_rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(format!("Response channel closed for '{}'", method)),
            Err(_) => {
                // Timeout — clean up pending
                let mut pending = self.pending.lock().await;
                pending.remove(&id);
                Err(format!("Request timed out: {} (id={})", method, id))
            }
        }
    }

    /// Process a single line received from stdin.
    async fn process_line(
        line: &str,
        pending: &Arc<Mutex<HashMap<u64, PendingRequest>>>,
        handler: &Arc<Mutex<Option<RequestHandler>>>,
    ) {
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                error!("Transport: failed to parse JSON: {}", e);
                // Try to send a parse error response if we can extract an id
                if let Some(id) = extract_id_from_line(line) {
                    Self::send_error(id, -32700, "Parse error");
                }
                return;
            }
        };

        let id = msg.get("id").and_then(|v| v.as_u64());
        let method = msg.get("method").and_then(|v| v.as_str());

        match (id, method) {
            // Notification (no id) — fire and forget
            (None, Some(_method_name)) => {
                debug!("Transport: received notification: {}", _method_name);
                // Notifications are currently ignored by the core
                // (they come from the extension, e.g., editor state changes)
            }
            // Response to one of our outgoing requests
            (Some(id_val), None) => {
                let mut pending_lock = pending.lock().await;
                if let Some(pending_req) = pending_lock.remove(&id_val) {
                    if let Some(error) = msg.get("error") {
                        let msg_str = error.get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error")
                            .to_string();
                        let _ = pending_req.response_tx.send(Err(msg_str));
                    } else {
                        let result = msg.get("result").cloned().unwrap_or(serde_json::Value::Null);
                        let _ = pending_req.response_tx.send(Ok(result));
                    }
                } else {
                    warn!("Transport: received response for unknown request id={}", id_val);
                }
            }
            // Incoming request from the extension
            (Some(id_val), Some(_method_name)) => {
                let handler_lock = handler.lock().await;
                if let Some(ref h) = *handler_lock {
                    // Pass the full message to the handler so it can extract method + params
                    let result = h(msg.clone());
                    Self::send_response(id_val, &result);
                } else {
                    warn!("Transport: no request handler registered, sending error for id={}", id_val);
                    Self::send_error(id_val, -32601, "Method not found: no handler registered");
                }
            }
            (None, None) => {
                warn!("Transport: received invalid message (no id and no method)");
            }
        }
    }

    /// Write a JSON message to stdout.
    fn write_json(value: &serde_json::Value) {
        let json = serde_json::to_string(value).unwrap_or_default();
        let mut stdout = io::stdout();
        writeln!(stdout, "{}", json).ok();
        stdout.flush().ok();
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        // The reader handle will be cancelled when the runtime shuts down
    }
}

/// Extract a numeric id from a raw JSON line (used for parse error responses).
fn extract_id_from_line(line: &str) -> Option<u64> {
    // Simple heuristic: look for "id":<number> or "id": <number>
    if let Some(pos) = line.find("\"id\"") {
        let after = &line[pos + 4..];
        // Skip colon and whitespace
        let after_colon = after.trim_start_matches(':').trim_start();
        if let Some(end) = after_colon.find(|c: char| !c.is_ascii_digit()) {
            after_colon[..end].parse().ok()
        } else {
            after_colon.parse().ok()
        }
    } else {
        None
    }
}
