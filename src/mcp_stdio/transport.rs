//! stdio transport layer.
//!
//! Handles stdin/stdout communication with MCP Server child processes.
//! Self-implemented instead of using rmcp's `TokioChildProcess` for:
//! - Lazy Loading lifecycle separation
//! - Health check integration
//! - Flexible error recovery

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    sync::{Mutex, RwLock, oneshot},
};

use super::types::{StdioRequest, StdioResponse};
use crate::{dual_debug, dual_error, dual_info, dual_warn};

/// Default request timeout in seconds.
const REQUEST_TIMEOUT_SECS: u64 = 30;

/// stdio transport layer.
///
/// Manages stdin/stdout communication with a single MCP Server child process.
/// Created by [`super::StdioProcessManager`] when a process is started.
pub struct StdioTransport {
    /// Process name (for logging)
    name: String,
    /// stdin handle for sending JSON-RPC requests
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    /// Pending request channels (request_id -> response sender)
    pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<StdioResponse>>>>,
    /// Whether transport is closed
    closed: Arc<RwLock<bool>>,
}

impl StdioTransport {
    /// Create a new transport from child process I/O handles.
    ///
    /// Spawns a background task to read and dispatch stdout responses.
    pub fn new(
        name: String,
        stdin: tokio::process::ChildStdin,
        stdout: tokio::process::ChildStdout,
    ) -> Self {
        let reader_name = name.clone();
        let transport = Self {
            name,
            stdin: Arc::new(Mutex::new(stdin)),
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            closed: Arc::new(RwLock::new(false)),
        };

        let pending = Arc::clone(&transport.pending_requests);
        let closed = Arc::clone(&transport.closed);
        tokio::spawn(async move {
            Self::read_stdout(reader_name, stdout, pending, closed).await;
        });

        transport
    }

    /// Send a JSON-RPC request and wait for a response.
    ///
    /// Generates a unique request ID, serializes the request to JSON,
    /// writes it to stdin, and waits for the corresponding response
    /// (with a 30-second timeout).
    pub async fn send_request(
        &self,
        method: String,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        if *self.closed.read().await {
            return Err("transport is closed".to_string());
        }

        // Generate unique request ID
        let id = uuid::Uuid::new_v4().to_string();

        // Build JSON-RPC 2.0 request
        let request = StdioRequest {
            jsonrpc: "2.0".to_string(),
            id: id.clone(),
            method,
            params,
        };

        // Create response channel
        let (tx, rx) = oneshot::channel();
        self.pending_requests.write().await.insert(id.clone(), tx);

        // Serialize and send to stdin
        let request_json = serde_json::to_string(&request)
            .map_err(|e| format!("failed to serialize request: {e}"))?;

        {
            let mut stdin = self.stdin.lock().await;
            let write_result = async {
                stdin.write_all(request_json.as_bytes()).await?;
                stdin.write_all(b"\n").await?;
                stdin.flush().await
            }
            .await;

            if let Err(e) = write_result {
                self.pending_requests.write().await.remove(&id);
                return Err(format!("failed to write to stdin: {e}"));
            }
        }

        dual_debug!("[mcp-stdio:{}] Sent request: {}", self.name, id);

        // Wait for response with timeout
        let result = tokio::time::timeout(Duration::from_secs(REQUEST_TIMEOUT_SECS), rx).await;

        let response = match result {
            Ok(Ok(response)) => response,
            Ok(Err(_)) => {
                self.pending_requests.write().await.remove(&id);
                return Err(format!("response channel closed: {id}"));
            }
            Err(_) => {
                self.pending_requests.write().await.remove(&id);
                return Err(format!(
                    "request timeout after {REQUEST_TIMEOUT_SECS}s: {id}"
                ));
            }
        };

        dual_debug!(
            "[mcp-stdio:{}] Received response: {}",
            self.name,
            response.id
        );

        // Process response
        if let Some(error) = response.error {
            Err(format!("MCP error {}: {}", error.code, error.message))
        } else if let Some(result) = response.result {
            Ok(result)
        } else {
            Err("empty response (no result or error)".to_string())
        }
    }

    /// Read stdout lines and dispatch JSON-RPC responses to pending requests.
    async fn read_stdout(
        name: String,
        stdout: tokio::process::ChildStdout,
        pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<StdioResponse>>>>,
        closed: Arc<RwLock<bool>>,
    ) {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => {
                    dual_info!("[mcp-stdio:{}] stdout EOF, transport closed", name);
                    *closed.write().await = true;
                    break;
                }
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }

                    match serde_json::from_str::<StdioResponse>(trimmed) {
                        Ok(response) => {
                            let id = response.id.clone();
                            let mut pending = pending_requests.write().await;
                            if let Some(tx) = pending.remove(&id) {
                                if tx.send(response).is_err() {
                                    dual_warn!(
                                        "[mcp-stdio:{}] Failed to deliver response for: {}",
                                        name,
                                        id
                                    );
                                }
                            } else {
                                dual_warn!(
                                    "[mcp-stdio:{}] Response for unknown request: {}",
                                    name,
                                    id
                                );
                            }
                        }
                        Err(e) => {
                            // Not all stdout lines are JSON-RPC responses
                            // (e.g., server might print logs to stdout)
                            dual_debug!(
                                "[mcp-stdio:{}] Non-JSON-RPC stdout: {} ({})",
                                name,
                                trimmed,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    dual_error!("[mcp-stdio:{}] stdout read error: {}", name, e);
                    *closed.write().await = true;
                    break;
                }
            }
        }

        // Cancel all pending requests when stdout closes
        pending_requests.write().await.clear();
    }

    /// Close the transport and drop all pending requests.
    pub async fn close(&self) {
        *self.closed.write().await = true;
        self.pending_requests.write().await.clear();
    }

    /// Check if the transport is closed.
    pub async fn is_closed(&self) -> bool {
        *self.closed.read().await
    }

    /// Get count of pending requests.
    pub async fn pending_count(&self) -> usize {
        self.pending_requests.read().await.len()
    }
}
