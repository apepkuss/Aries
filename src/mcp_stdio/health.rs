//! Health monitoring for stdio MCP Server processes.
//!
//! Performs periodic health checks via ping requests and tracks failure thresholds.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{sync::RwLock, time::interval};

use super::{
    transport::StdioTransport,
    types::{ProcessMetadata, ProcessStatus, StdioConfig},
};
use crate::{dual_debug, dual_error, dual_info, dual_warn};

/// Health monitor for stdio processes.
///
/// Manages periodic health check tasks for each running MCP Server process.
/// When consecutive failures exceed the configured threshold, the process
/// status is marked as `Failed`.
pub struct HealthMonitor {
    /// Active monitoring tasks (process name -> task handle)
    tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

impl HealthMonitor {
    /// Create a new health monitor.
    pub fn new() -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start health monitoring for a process.
    ///
    /// Stops any existing monitor for this process, then spawns a new
    /// monitoring loop task. If `health_check_interval_secs` is 0,
    /// monitoring is disabled and this method is a no-op.
    pub async fn start_monitoring(
        &self,
        name: String,
        metadata: Arc<RwLock<ProcessMetadata>>,
        transport: Arc<StdioTransport>,
        config: StdioConfig,
    ) {
        if config.health_check_interval_secs == 0 {
            dual_info!(
                "[mcp-stdio:{}] Health monitoring disabled (interval=0)",
                name
            );
            return;
        }

        // Stop existing monitor if any
        self.stop_monitoring(&name).await;

        dual_info!(
            "[mcp-stdio:{}] Starting health monitor (interval: {}s, threshold: {})",
            name,
            config.health_check_interval_secs,
            config.health_check_failure_threshold
        );

        let monitor_name = name.clone();
        let handle = tokio::spawn(async move {
            Self::monitor_loop(monitor_name, metadata, transport, config).await;
        });

        self.tasks.write().await.insert(name, handle);
    }

    /// Stop health monitoring for a process.
    pub async fn stop_monitoring(&self, name: &str) {
        if let Some(handle) = self.tasks.write().await.remove(name) {
            dual_info!("[mcp-stdio:{}] Stopping health monitor", name);
            handle.abort();
        }
    }

    /// Stop all health monitoring tasks.
    pub async fn stop_all(&self) {
        let mut tasks = self.tasks.write().await;
        for (name, handle) in tasks.drain() {
            dual_info!("[mcp-stdio:{}] Stopping health monitor", name);
            handle.abort();
        }
    }

    /// Health monitoring loop.
    ///
    /// Periodically performs health checks and updates process metadata.
    /// When consecutive failures reach the configured threshold, marks the
    /// process as `Failed` and exits the loop.
    async fn monitor_loop(
        name: String,
        metadata: Arc<RwLock<ProcessMetadata>>,
        transport: Arc<StdioTransport>,
        config: StdioConfig,
    ) {
        let mut ticker = interval(Duration::from_secs(config.health_check_interval_secs));

        // Skip the first immediate tick
        ticker.tick().await;

        loop {
            ticker.tick().await;

            match Self::perform_health_check(&name, &transport, &config).await {
                Ok(()) => {
                    let mut meta = metadata.write().await;
                    meta.last_health_check = Some(std::time::Instant::now());
                    meta.consecutive_failures = 0;
                    dual_debug!("[mcp-stdio:{}] Health check passed", name);
                }
                Err(e) => {
                    let mut meta = metadata.write().await;
                    meta.consecutive_failures += 1;

                    dual_warn!(
                        "[mcp-stdio:{}] Health check failed ({}/{}): {}",
                        name,
                        meta.consecutive_failures,
                        config.health_check_failure_threshold,
                        e
                    );

                    if meta.consecutive_failures >= config.health_check_failure_threshold {
                        dual_error!(
                            "[mcp-stdio:{}] Health check failure threshold reached",
                            name
                        );
                        meta.status = ProcessStatus::Failed(e);
                        // TODO: Phase 5 — trigger recovery manager
                        return;
                    }
                }
            }
        }
    }

    /// Perform a single health check.
    ///
    /// Checks if the transport is still open, then sends a JSON-RPC `ping`
    /// request with a timeout derived from the stdio config.
    async fn perform_health_check(
        name: &str,
        transport: &StdioTransport,
        config: &StdioConfig,
    ) -> Result<(), String> {
        if transport.is_closed().await {
            return Err("transport is closed".to_string());
        }

        let result = tokio::time::timeout(
            Duration::from_secs(config.health_check_timeout_secs),
            transport.send_request("ping".to_string(), serde_json::json!({})),
        )
        .await;

        match result {
            Ok(Ok(_)) => {
                dual_debug!("[mcp-stdio:{}] Ping successful", name);
                Ok(())
            }
            Ok(Err(e)) => Err(format!("ping failed: {e}")),
            Err(_) => Err(format!(
                "ping timeout after {}s",
                config.health_check_timeout_secs
            )),
        }
    }
}

impl Default for HealthMonitor {
    fn default() -> Self {
        Self::new()
    }
}
