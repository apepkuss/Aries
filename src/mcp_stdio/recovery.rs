//! Recovery management for stdio MCP Server processes.
//!
//! Handles crash detection and automatic restart based on StdioConfig policy.

// TODO: Phase 5 implementation

/// Recovery manager for stdio processes.
pub struct RecoveryManager;

/// Recovery action to take after process failure.
pub enum RecoveryAction {
    /// Do nothing
    None,
    /// Restart process after delay
    Restart {
        /// Delay in seconds before restart
        delay_secs: u64,
    },
    /// Give up recovery
    GiveUp,
}
