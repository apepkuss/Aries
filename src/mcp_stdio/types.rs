//! Type definitions for stdio MCP transport.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// stdio process configuration (top-level fields from config.toml).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StdioProcessConfig {
    /// Service name
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Working directory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Whether enabled
    #[serde(default = "default_enable")]
    pub enable: bool,
    /// stdio-specific configuration (maps to [mcp.server.tool.stdio] section)
    #[serde(default)]
    pub stdio: StdioConfig,
}

fn default_enable() -> bool {
    true
}

/// stdio-specific configuration (flat structure, maps to [mcp.server.tool.stdio]).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StdioConfig {
    /// Health check interval in seconds, 0 to disable
    #[serde(default = "default_interval")]
    pub health_check_interval_secs: u64,
    /// Health check timeout in seconds
    #[serde(default = "default_timeout")]
    pub health_check_timeout_secs: u64,
    /// Consecutive health check failure threshold
    #[serde(default = "default_failure_threshold")]
    pub health_check_failure_threshold: u32,
    /// Whether to auto-restart on failure
    #[serde(default = "default_restart_on_failure")]
    pub restart_on_failure: bool,
    /// Maximum restart attempts
    #[serde(default = "default_max_restart")]
    pub max_restart_attempts: u32,
    /// Restart backoff interval in seconds
    #[serde(default = "default_backoff")]
    pub restart_backoff_secs: u64,
}

fn default_interval() -> u64 {
    30
}
fn default_timeout() -> u64 {
    5
}
fn default_failure_threshold() -> u32 {
    3
}
fn default_restart_on_failure() -> bool {
    true
}
fn default_max_restart() -> u32 {
    3
}
fn default_backoff() -> u64 {
    5
}

impl Default for StdioConfig {
    fn default() -> Self {
        Self {
            health_check_interval_secs: 30,
            health_check_timeout_secs: 5,
            health_check_failure_threshold: 3,
            restart_on_failure: true,
            max_restart_attempts: 3,
            restart_backoff_secs: 5,
        }
    }
}

/// Process status.
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessStatus {
    /// Starting up
    Starting,
    /// Running normally
    Running,
    /// Shutting down
    Stopping,
    /// Stopped
    Stopped,
    /// Failed with error message
    Failed(String),
    /// Restarting
    Restarting,
}

/// Process metadata.
#[derive(Debug, Clone)]
pub struct ProcessMetadata {
    /// Process name
    pub name: String,
    /// Process ID
    pub pid: Option<u32>,
    /// Current status
    pub status: ProcessStatus,
    /// Start time
    pub start_time: Option<std::time::Instant>,
    /// Restart count
    pub restart_count: u32,
    /// Last health check time
    pub last_health_check: Option<std::time::Instant>,
    /// Consecutive failure count
    pub consecutive_failures: u32,
}

/// stdio JSON-RPC request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// stdio JSON-RPC response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<StdioError>,
}

/// stdio JSON-RPC error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StdioError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}
