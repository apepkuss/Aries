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

pub use health::HealthMonitor;
pub use manager::StdioProcessManager;
pub use recovery::{RecoveryAction, RecoveryManager};
pub use transport::StdioTransport;
pub use types::*;
