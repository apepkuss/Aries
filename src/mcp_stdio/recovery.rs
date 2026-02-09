//! Recovery management for stdio MCP Server processes.
//!
//! Handles crash detection and automatic restart decisions based on StdioConfig policy.
//! Actual restarts are performed via Lazy Loading in [`super::StdioProcessManager::get_transport`].

use std::collections::{HashMap, HashSet};

use tokio::sync::RwLock;

use super::types::StdioProcessConfig;
use crate::{dual_error, dual_info};

/// Recovery manager for stdio processes.
///
/// Tracks restart counts across process lifecycles and determines recovery
/// actions based on [`StdioProcessConfig`] policy. Restart counts persist
/// even after the process is removed from the active processes map, ensuring
/// `max_restart_attempts` is respected across crash-restart cycles.
pub struct RecoveryManager {
    /// Restart counts per process (name -> count)
    restart_counts: RwLock<HashMap<String, u32>>,
    /// Processes that have exceeded max restart attempts
    gave_up_processes: RwLock<HashSet<String>>,
}

impl RecoveryManager {
    /// Create a new recovery manager.
    pub fn new() -> Self {
        Self {
            restart_counts: RwLock::new(HashMap::new()),
            gave_up_processes: RwLock::new(HashSet::new()),
        }
    }

    /// Evaluate recovery action for a failed process.
    ///
    /// Based on the process configuration:
    /// - `restart_on_failure = false` → [`RecoveryAction::None`]
    /// - Restart count >= `max_restart_attempts` → [`RecoveryAction::GiveUp`]
    /// - Otherwise → [`RecoveryAction::Restart`] with backoff delay
    pub async fn handle_process_failure(
        &self,
        name: &str,
        config: &StdioProcessConfig,
    ) -> RecoveryAction {
        let stdio_config = &config.stdio;

        if !stdio_config.restart_on_failure {
            dual_info!(
                "[mcp-stdio:{}] restart_on_failure is disabled, not restarting",
                name
            );
            return RecoveryAction::None;
        }

        let restart_count = self.get_restart_count(name).await;

        if restart_count >= stdio_config.max_restart_attempts {
            dual_error!(
                "[mcp-stdio:{}] Max restart attempts ({}) reached, giving up",
                name,
                stdio_config.max_restart_attempts
            );
            self.gave_up_processes
                .write()
                .await
                .insert(name.to_string());
            return RecoveryAction::GiveUp;
        }

        dual_info!(
            "[mcp-stdio:{}] Scheduling restart (attempt {}/{})",
            name,
            restart_count + 1,
            stdio_config.max_restart_attempts
        );

        RecoveryAction::Restart {
            delay_secs: stdio_config.restart_backoff_secs,
        }
    }

    /// Record a restart attempt (increment counter).
    ///
    /// Should be called when a process failure is detected, before
    /// calling [`handle_process_failure`](Self::handle_process_failure).
    pub async fn record_restart(&self, name: &str) {
        let mut counts = self.restart_counts.write().await;
        let count = counts.entry(name.to_string()).or_insert(0);
        *count += 1;
    }

    /// Get the current restart count for a process.
    pub async fn get_restart_count(&self, name: &str) -> u32 {
        self.restart_counts
            .read()
            .await
            .get(name)
            .copied()
            .unwrap_or(0)
    }

    /// Check if recovery has given up for a process.
    pub async fn has_given_up(&self, name: &str) -> bool {
        self.gave_up_processes.read().await.contains(name)
    }

    /// Reset recovery state for a process.
    ///
    /// Clears restart count and removes from gave-up set,
    /// allowing the process to be restarted again.
    pub async fn reset(&self, name: &str) {
        self.restart_counts.write().await.remove(name);
        self.gave_up_processes.write().await.remove(name);
    }
}

impl Default for RecoveryManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Recovery action to take after process failure.
#[derive(Debug, PartialEq)]
pub enum RecoveryAction {
    /// Do nothing (restart disabled)
    None,
    /// Restart process after delay
    Restart {
        /// Delay in seconds before restart
        delay_secs: u64,
    },
    /// Give up recovery (max attempts exceeded)
    GiveUp,
}
