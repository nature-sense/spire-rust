// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (c) 2026 NatureSense

//! JSON-RPC 2.0 transport over TCP socket.
//!
//! This module provides bidirectional JSON-RPC communication over a TCP
//! loopback socket, replacing the previous stdin/stdout transport which
//! suffered from spurious EOF issues when the process was spawned by Node.js.
//!
//! Architecture:
//!   - Core binds to 127.0.0.1:0 (OS-assigned port)
//!   - Core prints "SPIRE_PORT=<port>" to stdout for the extension to read
//!   - Extension connects to 127.0.0.1:<port>
//!   - All JSON-RPC messages flow over the TCP connection
//!
//! Protocol: https://www.jsonrpc.org/specification
//!
//! Messages (newline-delimited JSON):
//!   {"jsonrpc":"2.0","id":1,"method":"chat/getActive","params":{}}
//!   {"jsonrpc":"2.0","id":1,"result":{...}}
//!   {"jsonrpc":"2.0","method":"event/chat/message","params":{...}}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, error, info, warn};

/// A pending outgoing request (core → extension) waiting for a response.
struct PendingRequest {
    response_tx: oneshot::Sender<Result<serde_json::Value, String>>,
    #[allow(dead_code)]
    method: String,
}

/// Handler for incoming JSON-RPC requests from the extension.
pub type RequestHandler = Arc<dyn Fn(serde_json::Value) -> serde_json::Value + Send + Sync>;

/// Bidirectional JSON-RPC 2.0 transport over TCP socket.
pub struct Transport {
    /// Pending outgoing requests awaiting responses.
    pending: Arc<Mutex<HashMap<u64, PendingRequest>>>,
    /// Next request ID for outgoing requests.
    next_id: Arc<Mutex<u64>>,
    /// Handler for incoming requests from the extension.
    request_handler: Arc<Mutex<Option<RequestHandler>>>,
    /// The port the transport is listening on.
    port: Arc<Mutex<Option<u16>>>,
    /// The TCP listener (held to keep the port bound).
    _listener: Option<TcpListener>,
    /// Write half of the accepted connection.
    writer: Arc<Mutex<Option<tokio::io::WriteHalf<TcpStream>>>>,
    /// Join handle for the socket reader task.
    _reader_handle: Option<tokio::task::JoinHandle<()>>,
}

impl Transport {
    /// Create a new transport.
    ///
    /// The listener is NOT started until `start()` is called.
    /// This allows the caller to set up the request handler first,
    /// avoiding race conditions where requests arrive before the
    /// handler is registered.
    pub fn new() -> Self {
        let pending = Arc::new(Mutex::new(HashMap::new()));
        let next_id = Arc::new(Mutex::new(1u64));
        let request_handler: Arc<Mutex<Option<RequestHandler>>> = Arc::new(Mutex::new(None));
        let port = Arc::new(Mutex::new(None));
        let writer = Arc::new(Mutex::new(None));

        Self {
            pending,
            next_id,
            request_handler,
            port,
            _listener: None,
            writer,
            _reader_handle: None,
        }
    }

    /// Get the port the transport is listening on.
    /// Returns `None` if `start()` has not been called yet.
    pub async fn port(&self) -> Option<u16> {
        *self.port.lock().await
    }

    /// Set the handler for incoming requests from the extension.
    pub async fn set_request_handler(&self, handler: RequestHandler) {
        let mut h = self.request_handler.lock().await;
        *h = Some(handler);
    }

    /// Bind the TCP listener to a loopback port.
    ///
    /// This binds the listener but does NOT accept connections yet.
    /// Call `accept()` later to wait for the extension to connect.
    ///
    /// Returns the port number the listener is bound to.
    pub async fn bind(&mut self) -> Result<u16, Box<dyn std::error::Error>> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_addr = listener.local_addr()?;
        let port = local_addr.port();

        info!("Transport: listening on 127.0.0.1:{}", port);

        // Store the port
        {
            let mut p = self.port.lock().await;
            *p = Some(port);
        }

        self._listener = Some(listener);

        Ok(port)
    }

    /// Accept one connection from the bound listener.
    ///
    /// Must be called AFTER `bind()` and `set_request_handler()`.
    /// This will block until the extension connects.
    pub async fn accept(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let listener = self._listener.as_ref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotConnected, "Transport not bound yet")
        })?;

        let pending = self.pending.clone();
        let handler = self.request_handler.clone();
        let writer = self.writer.clone();

        // Accept one connection
        let (stream, peer_addr) = listener.accept().await?;
        info!("Transport: accepted connection from {}", peer_addr);

        // Split the stream into read/write halves
        let (reader, writer_half) = tokio::io::split(stream);

        // Store the writer half for outgoing messages
        {
            let mut w = writer.lock().await;
            *w = Some(writer_half);
        }

        // Spawn a task to read lines from the socket and process them.
        let reader_handle = tokio::spawn(async move {
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();
            loop {
                line.clear();
                match buf_reader.read_line(&mut line).await {
                    Ok(0) => {
                        info!("Transport: socket EOF (extension closed connection)");
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim().to_string();
                        if !trimmed.is_empty() {
                            Self::process_line(&trimmed, &pending, &handler, &writer).await;
                        }
                    }
                    Err(e) => {
                        error!("Transport: error reading from socket: {}", e);
                        break;
                    }
                }
            }
            info!("Transport: socket reader task finished");
        });

        self._reader_handle = Some(reader_handle);

        Ok(())
    }

    /// Start the TCP listener and accept one connection (original combined method).
    ///
    /// Equivalent to calling `bind()` then `accept()`.
    /// Returns the port number the listener is bound to.
    pub async fn start(&mut self) -> Result<u16, Box<dyn std::error::Error>> {
        let port = self.bind().await?;
        self.accept().await?;
        Ok(port)
    }

    /// Send a JSON-RPC response to the extension.
    pub async fn send_response(&self, id: u64, result: &serde_json::Value) {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        });
        self.write_json(&response).await;
    }

    /// Send a JSON-RPC error response to the extension.
    pub async fn send_error(&self, id: u64, code: i64, message: &str) {
        let response = serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {
                "code": code,
                "message": message,
            },
        });
        self.write_json(&response).await;
    }

    /// Send a notification (event) to the extension.
    pub async fn send_notification(&self, method: &str, params: &serde_json::Value) {
        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        });
        self.write_json(&notification).await;
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
        self.write_json(&request).await;

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

    /// Process a single line received from the socket.
    async fn process_line(
        line: &str,
        pending: &Arc<Mutex<HashMap<u64, PendingRequest>>>,
        handler: &Arc<Mutex<Option<RequestHandler>>>,
        writer: &Arc<Mutex<Option<tokio::io::WriteHalf<TcpStream>>>>,
    ) {
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                error!("Transport: failed to parse JSON: {}", e);
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
            //
            // IMPORTANT: The handler must be executed on a background task, NOT
            // on the reader task. If the handler calls call_extension() (which
            // writes to the socket and waits for a response), the reader task
            // must be alive to read that response. Blocking the reader task
            // would cause a deadlock.
            (Some(id_val), Some(_method_name)) => {
                let handler_clone = handler.clone();
                let writer_clone = writer.clone();
                let msg_clone = msg.clone();
                tokio::spawn(async move {
                    let handler_lock = handler_clone.lock().await;
                    if let Some(ref h) = *handler_lock {
                        let result = h(msg_clone);
                        drop(handler_lock);

                        // Send the response back through the socket
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id_val,
                            "result": result,
                        });
                        let json = serde_json::to_string(&response).unwrap_or_default();
                        let mut w = writer_clone.lock().await;
                        if let Some(ref mut writer_half) = *w {
                            let line = format!("{}\n", json);
                            if let Err(e) = writer_half.write_all(line.as_bytes()).await {
                                error!("Transport: failed to write response to socket: {}", e);
                            }
                        } else {
                            error!("Transport: cannot write response, socket writer not available");
                        }
                    } else {
                        warn!("Transport: no request handler registered, sending error for id={}", id_val);
                        let response = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id_val,
                            "error": {
                                "code": -32601,
                                "message": "Method not found: no handler registered",
                            },
                        });
                        let json = serde_json::to_string(&response).unwrap_or_default();
                        let mut w = writer_clone.lock().await;
                        if let Some(ref mut writer_half) = *w {
                            let line = format!("{}\n", json);
                            let _ = writer_half.write_all(line.as_bytes()).await;
                        }
                    }
                });
            }
            (None, None) => {
                warn!("Transport: received invalid message (no id and no method)");
            }
        }
    }

    /// Write a JSON message to the socket.
    async fn write_json(&self, value: &serde_json::Value) {
        let json = serde_json::to_string(value).unwrap_or_default();
        let mut writer = self.writer.lock().await;
        if let Some(ref mut w) = *writer {
            let line = format!("{}\n", json);
            if let Err(e) = w.write_all(line.as_bytes()).await {
                warn!("Transport: failed to write to socket: {}", e);
            }
        } else {
            debug!("Transport: cannot write JSON, socket writer not available (not yet connected)");
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        // The reader handle will be cancelled when the runtime shuts down
    }
}
