//! stdio transport layer.
//!
//! Handles stdin/stdout communication with MCP Server child processes.
//! Self-implemented instead of using rmcp's `TokioChildProcess` for:
//! - Lazy Loading lifecycle separation
//! - Health check integration
//! - Flexible error recovery

use std::{collections::HashMap, sync::Arc};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    sync::{Mutex, RwLock, oneshot},
};

use super::types::StdioResponse;
use crate::{dual_debug, dual_error, dual_info};

/// stdio transport layer.
///
/// Manages stdin/stdout communication with a single MCP Server child process.
/// Created by [`super::StdioProcessManager`] when a process is started.
pub struct StdioTransport {
    /// Process name (for logging)
    // TODO: Phase 3 — used in send_request()
    #[allow(dead_code)]
    name: String,
    /// stdin handle for sending JSON-RPC requests
    // TODO: Phase 3 — used in send_request()
    #[allow(dead_code)]
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    /// Pending request channels (request_id -> response sender)
    pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<StdioResponse>>>>,
    /// Whether transport is closed
    closed: Arc<RwLock<bool>>,
}

impl StdioTransport {
    /// Create a new transport from child process I/O handles.
    ///
    /// Spawns a background task to read stdout.
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

    /// Read stdout lines from the child process.
    ///
    /// TODO: Phase 3 — parse JSON-RPC responses and dispatch to pending requests.
    async fn read_stdout(
        name: String,
        stdout: tokio::process::ChildStdout,
        _pending_requests: Arc<RwLock<HashMap<String, oneshot::Sender<StdioResponse>>>>,
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
                    // TODO: Phase 3 — parse JSON-RPC response and dispatch
                    dual_debug!("[mcp-stdio:{}] stdout: {}", name, line.trim());
                }
                Err(e) => {
                    dual_error!("[mcp-stdio:{}] stdout read error: {}", name, e);
                    *closed.write().await = true;
                    break;
                }
            }
        }
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
