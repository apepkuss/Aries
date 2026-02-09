//! Health monitoring for stdio MCP Server processes.
//!
//! Performs periodic health checks via `RunningService.list_all_tools()`
//! through `MCP_SERVICES` and tracks failure thresholds.
//! On failure threshold, sends restart request via channel to avoid
//! async type cycles with `StdioProcessManager`.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    sync::{RwLock, mpsc},
    time::interval,
};

use super::{
    recovery::{RecoveryAction, RecoveryManager},
    types::{ProcessMetadata, ProcessStatus, StdioConfig, StdioProcessConfig},
};
use crate::{dual_debug, dual_error, dual_info, dual_warn, mcp::MCP_SERVICES};

/// Health monitor for stdio processes.
///
/// Manages periodic health check tasks for each running MCP Server process.
/// Health checks are performed via `MCP_SERVICES` (calling `list_all_tools()`
/// on the `RunningService`). When consecutive failures exceed the configured
/// threshold, a restart request is sent via `restart_tx` channel.
pub struct HealthMonitor {
    /// Active monitoring tasks (process name -> task handle)
    tasks: Arc<RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
    /// Channel sender for restart requests
    restart_tx: mpsc::Sender<String>,
}

impl HealthMonitor {
    /// Create a new health monitor with a restart request channel.
    pub fn new(restart_tx: mpsc::Sender<String>) -> Self {
        Self {
            tasks: Arc::new(RwLock::new(HashMap::new())),
            restart_tx,
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
        service_name: String,
        config: StdioConfig,
        process_config: StdioProcessConfig,
        recovery_manager: Arc<RecoveryManager>,
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

        let restart_tx = self.restart_tx.clone();
        let monitor_name = name.clone();
        let handle = tokio::spawn(async move {
            Self::monitor_loop(
                monitor_name,
                metadata,
                service_name,
                config,
                process_config,
                recovery_manager,
                restart_tx,
            )
            .await;
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
    /// Periodically performs health checks via `MCP_SERVICES` and updates
    /// process metadata. When consecutive failures reach the configured
    /// threshold, sends a restart request via channel.
    async fn monitor_loop(
        name: String,
        metadata: Arc<RwLock<ProcessMetadata>>,
        service_name: String,
        config: StdioConfig,
        process_config: StdioProcessConfig,
        recovery_manager: Arc<RecoveryManager>,
        restart_tx: mpsc::Sender<String>,
    ) {
        let mut ticker = interval(Duration::from_secs(config.health_check_interval_secs));

        // Skip the first immediate tick
        ticker.tick().await;

        loop {
            ticker.tick().await;

            match Self::perform_health_check(&name, &service_name, &config).await {
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
                        drop(meta); // Release lock before recovery

                        // Evaluate recovery action
                        recovery_manager.record_restart(&name).await;
                        let action = recovery_manager
                            .handle_process_failure(&name, &process_config)
                            .await;

                        match action {
                            RecoveryAction::Restart { .. } => {
                                dual_info!(
                                    "[mcp-stdio:{}] Requesting automatic restart via channel",
                                    name
                                );
                                if let Err(e) = restart_tx.send(name.clone()).await {
                                    dual_error!(
                                        "[mcp-stdio:{}] Failed to send restart request: {}",
                                        name,
                                        e
                                    );
                                }
                            }
                            RecoveryAction::GiveUp => {
                                dual_error!(
                                    "[mcp-stdio:{}] Recovery gave up after health check failures",
                                    name
                                );
                            }
                            RecoveryAction::None => {
                                dual_info!(
                                    "[mcp-stdio:{}] Recovery disabled, process will not restart",
                                    name
                                );
                            }
                        }

                        return;
                    }
                }
            }
        }
    }

    /// Perform a single health check via MCP_SERVICES.
    ///
    /// Looks up the `RunningService` by service_name and calls
    /// `list_all_tools()` with a timeout.
    async fn perform_health_check(
        name: &str,
        service_name: &str,
        config: &StdioConfig,
    ) -> Result<(), String> {
        let services = MCP_SERVICES
            .get()
            .ok_or_else(|| "MCP_SERVICES not initialized".to_string())?;

        let service_map = services.read().await;
        let service_lock = service_map
            .get(service_name)
            .ok_or_else(|| format!("service '{service_name}' not found in MCP_SERVICES"))?;

        let service = service_lock.read().await;

        let result = tokio::time::timeout(
            Duration::from_secs(config.health_check_timeout_secs),
            service.raw.list_all_tools(),
        )
        .await;

        match result {
            Ok(Ok(_tools)) => {
                dual_debug!("[mcp-stdio:{}] Health check successful", name);
                Ok(())
            }
            Ok(Err(e)) => Err(format!("list_all_tools failed: {e}")),
            Err(_) => Err(format!(
                "health check timeout after {}s",
                config.health_check_timeout_secs
            )),
        }
    }
}
