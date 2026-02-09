//! stdio MCP Transport Support
//!
//! Provides full stdio MCP Server process management, health checking,
//! and recovery mechanisms for Aries Agent.
//!
//! # Architecture
//!
//! ```text
//! +-----------------------------------------------------------+
//! |                  StdioProcessManager                       |
//! |  +-------------+  +---------------+  +-----------------+  |
//! |  | Transport   |  | HealthMonitor |  | RecoveryManager |  |
//! |  | (stdin/out) |  | (ping check)  |  | (auto restart)  |  |
//! |  +-------------+  +---------------+  +-----------------+  |
//! |        |                  |                  |             |
//! |        v                  v                  v             |
//! |   Child Process     Process State      Restart Policy     |
//! +-----------------------------------------------------------+
//! ```
//!
//! # Design Decisions
//!
//! - **Lazy Loading**: Processes start on first tool call, not at system startup
//! - **No Sandbox**: Processes run with Aries Agent's permissions (user's responsibility)
//! - **Self-implemented Transport**: Not using rmcp's `TokioChildProcess` for lifecycle control

mod health;
mod manager;
mod recovery;
mod transport;
mod types;

#[cfg(test)]
mod tests;

use std::sync::Arc;

pub use health::HealthMonitor;
pub use manager::StdioProcessManager;
use once_cell::sync::OnceCell;
pub use recovery::{RecoveryAction, RecoveryManager};
pub use transport::StdioTransport;
pub use types::*;

/// Global stdio process manager instance.
pub static STDIO_PROCESS_MANAGER: OnceCell<Arc<StdioProcessManager>> = OnceCell::new();

/// Get or initialize the global stdio process manager.
pub fn get_stdio_process_manager() -> &'static Arc<StdioProcessManager> {
    STDIO_PROCESS_MANAGER.get_or_init(|| Arc::new(StdioProcessManager::new()))
}
