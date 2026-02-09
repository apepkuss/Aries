//! stdio MCP Server Process Management
//!
//! Manages lifecycle of stdio MCP Server child processes using rmcp's
//! `TokioChildProcess`. Processes produce `RunningService` registered
//! into `MCP_SERVICES` (unified with SSE/StreamHTTP).
//!
//! # Architecture
//!
//! ```text
//! +-----------------------------------------------------------+
//! |                  StdioProcessManager                       |
//! |  +------------------+  +---------------+  +-------------+ |
//! |  | TokioChildProcess|  | HealthMonitor |  | Recovery    | |
//! |  | → RunningService |  | (list_tools)  |  | Manager     | |
//! |  +------------------+  +---------------+  +-------------+ |
//! |        |                      |                 |          |
//! |        v                      v                 v          |
//! |   MCP_SERVICES          Process State     Restart Policy   |
//! +-----------------------------------------------------------+
//! ```
//!
//! # Design Decisions
//!
//! - **Unified Architecture**: Uses rmcp `TokioChildProcess` to produce `RunningService`,
//!   registered into `MCP_SERVICES` (same as SSE/StreamHTTP)
//! - **Startup at Config Load**: Processes start during config load (not lazy loading)
//! - **No Sandbox**: Processes run with Aries Agent's permissions (user's responsibility)

mod health;
mod manager;
mod recovery;
mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use health::HealthMonitor;
pub use manager::StdioProcessManager;
use once_cell::sync::OnceCell;
pub use recovery::{RecoveryAction, RecoveryManager};
use tokio::io::{AsyncBufReadExt, BufReader};
pub use types::*;

use crate::{dual_error, dual_warn};

/// Global stdio process manager instance.
pub static STDIO_PROCESS_MANAGER: OnceCell<Arc<StdioProcessManager>> = OnceCell::new();

/// Get or initialize the global stdio process manager.
pub fn get_stdio_process_manager() -> &'static Arc<StdioProcessManager> {
    STDIO_PROCESS_MANAGER.get_or_init(|| Arc::new(StdioProcessManager::new()))
}

/// Log stderr output from a child process.
///
/// Reads lines from the child's stderr and logs them as warnings.
/// This function runs until EOF or an error occurs.
pub async fn log_stderr(name: String, stderr: tokio::process::ChildStderr) {
    let mut reader = BufReader::new(stderr);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break,
            Ok(_) => {
                dual_warn!("[mcp-stdio:{}] stderr: {}", name, line.trim());
            }
            Err(e) => {
                dual_error!("[mcp-stdio:{}] stderr read error: {}", name, e);
                break;
            }
        }
    }
}
