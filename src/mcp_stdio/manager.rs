//! stdio process manager.
//!
//! Manages lifecycle of stdio MCP Server child processes.
//! Processes are started via rmcp's `TokioChildProcess`, producing
//! `RunningService` registered into `MCP_SERVICES` (unified with SSE/StreamHTTP).
//! Health monitoring and recovery are managed as an additional layer.

use std::{collections::HashMap, sync::Arc, time::Duration};

use rmcp::{
    model::{ClientCapabilities, ClientInfo, Implementation},
    service::ServiceExt,
    transport::TokioChildProcess,
};
use tokio::sync::{RwLock, mpsc};

use super::{
    health::HealthMonitor,
    recovery::RecoveryManager,
    types::{ProcessMetadata, ProcessStatus, StdioProcessConfig},
};
use crate::{
    dual_error, dual_info, dual_warn,
    mcp::{MCP_SERVICES, McpService},
};

/// Stdio process lifecycle info.
struct StdioProcessInfo {
    /// Process metadata (PID, status, restart count)
    metadata: Arc<RwLock<ProcessMetadata>>,
    /// Service name in MCP_SERVICES
    service_name: String,
    /// Fallback message for the MCP service
    fallback_message: Option<String>,
}

/// stdio process manager.
///
/// Manages lifecycle of stdio MCP Server child processes.
/// Processes are started via `TokioChildProcess` and registered into
/// `MCP_SERVICES` (same as SSE/StreamHTTP). This manager provides
/// health monitoring and recovery as an additional layer.
pub struct StdioProcessManager {
    /// Process configurations (name -> config)
    configs: Arc<RwLock<HashMap<String, StdioProcessConfig>>>,
    /// Registered stdio processes (name -> process info)
    processes: Arc<RwLock<HashMap<String, Arc<StdioProcessInfo>>>>,
    /// Health monitor
    health_monitor: Arc<HealthMonitor>,
    /// Recovery manager
    recovery_manager: Arc<RecoveryManager>,
    /// Restart receiver (taken once to spawn the listener task)
    restart_rx: std::sync::Mutex<Option<mpsc::Receiver<String>>>,
}

impl StdioProcessManager {
    /// Create a new process manager.
    ///
    /// Creates a channel for restart requests from the health monitor.
    /// The restart listener is spawned lazily when the first process
    /// is registered (requires a Tokio runtime).
    pub fn new() -> Self {
        let (restart_tx, restart_rx) = mpsc::channel::<String>(16);

        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(RwLock::new(HashMap::new())),
            health_monitor: Arc::new(HealthMonitor::new(restart_tx)),
            recovery_manager: Arc::new(RecoveryManager::new()),
            restart_rx: std::sync::Mutex::new(Some(restart_rx)),
        }
    }

    /// Ensure the restart listener task is spawned.
    ///
    /// Takes the receiver from the mutex and spawns the listener.
    /// Safe to call multiple times; only the first call spawns the task.
    fn ensure_restart_listener(&self) {
        if let Ok(mut guard) = self.restart_rx.lock()
            && let Some(rx) = guard.take()
        {
            tokio::spawn(Self::restart_listener(rx));
        }
    }

    /// Register a process configuration.
    pub async fn register_config(&self, config: StdioProcessConfig) {
        let name = config.name.clone();
        dual_info!("[mcp-stdio] Registered config for '{}'", name);
        self.configs.write().await.insert(name, config);
    }

    /// Register a running stdio process for lifecycle management.
    ///
    /// Called by `connect_stdio()` after successfully starting the process
    /// and registering its `RunningService` into `MCP_SERVICES`.
    /// Starts health monitoring for the registered process.
    pub async fn register_process(
        &self,
        name: &str,
        service_name: String,
        pid: Option<u32>,
        fallback_message: Option<String>,
    ) {
        self.ensure_restart_listener();

        let config = {
            let configs = self.configs.read().await;
            configs.get(name).cloned().unwrap_or_else(|| {
                dual_warn!(
                    "[mcp-stdio:{}] Config not found during register, using defaults",
                    name
                );
                StdioProcessConfig {
                    name: name.to_string(),
                    command: String::new(),
                    args: vec![],
                    env: HashMap::new(),
                    working_dir: None,
                    enable: true,
                    stdio: Default::default(),
                }
            })
        };

        let metadata = Arc::new(RwLock::new(ProcessMetadata::new(name, pid)));
        {
            let mut meta = metadata.write().await;
            meta.status = ProcessStatus::Running;
        }

        let info = Arc::new(StdioProcessInfo {
            metadata: Arc::clone(&metadata),
            service_name: service_name.clone(),
            fallback_message,
        });

        self.processes.write().await.insert(name.to_string(), info);

        // Start health monitoring
        self.health_monitor
            .start_monitoring(
                name.to_string(),
                metadata,
                service_name,
                config.stdio.clone(),
                config.clone(),
                Arc::clone(&self.recovery_manager),
            )
            .await;

        dual_info!(
            "[mcp-stdio:{}] Process registered for lifecycle management",
            name
        );
    }

    /// Start a process from registered config.
    ///
    /// Creates a new `TokioChildProcess`, performs MCP handshake,
    /// discovers tools, and registers the `RunningService` into `MCP_SERVICES`.
    /// Used for restart scenarios (initial startup is done by `connect_stdio()`).
    pub async fn start_process(&self, name: &str) -> Result<(), String> {
        self.ensure_restart_listener();

        let config = {
            let configs = self.configs.read().await;
            configs
                .get(name)
                .cloned()
                .ok_or_else(|| format!("config not found for '{name}'"))?
        };

        // Check if already registered
        if self.processes.read().await.contains_key(name) {
            return Err(format!("process '{name}' is already registered"));
        }

        dual_warn!(
            "[mcp-stdio:{}] Starting child process '{}' with Aries Agent privileges. \
             Ensure you trust this MCP server.",
            name,
            config.command
        );

        // Build command
        let mut cmd = tokio::process::Command::new(&config.command);
        cmd.args(&config.args);
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }

        // Spawn via TokioChildProcess
        let (child_process, stderr) = TokioChildProcess::builder(cmd)
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to start process '{name}': {e}"))?;

        let pid = child_process.id();
        dual_info!("[mcp-stdio:{}] Process started (PID: {:?})", name, pid);

        // Spawn stderr logger
        if let Some(stderr) = stderr {
            let log_name = name.to_string();
            tokio::spawn(async move {
                super::log_stderr(log_name, stderr).await;
            });
        }

        // MCP handshake
        let client_info = ClientInfo {
            protocol_version: Default::default(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: env!("CARGO_PKG_NAME").to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
        };

        let service = client_info
            .into_dyn()
            .serve(child_process)
            .await
            .map_err(|e| format!("failed to connect to stdio MCP server '{name}': {e}"))?;

        // Get service name
        let service_name = match service.peer_info() {
            Some(peer_info) => peer_info.server_info.name.clone(),
            None => name.to_string(),
        };

        // Discover tools
        let tools = service
            .list_all_tools()
            .await
            .map_err(|e| format!("failed to list tools from '{name}': {e}"))?;

        dual_info!("Found {} tools from {} stdio mcp server", tools.len(), name,);

        // Get fallback_message from previous process info (if restarting)
        let fallback_message = {
            let processes = self.processes.read().await;
            processes
                .get(name)
                .and_then(|info| info.fallback_message.clone())
        };

        // Create McpService and register to MCP_SERVICES
        let mut client = McpService::new(&service_name, service);
        client.tools = tools.iter().map(|tool| tool.name.to_string()).collect();
        client.fallback_message = fallback_message.clone();

        match MCP_SERVICES.get() {
            Some(clients) => {
                let mut clients = clients.write().await;
                // Remove existing entry if present (restart case)
                clients.remove(&service_name);
                clients.insert(service_name.clone(), tokio::sync::RwLock::new(client));
            }
            None => {
                MCP_SERVICES
                    .set(tokio::sync::RwLock::new(HashMap::from([(
                        service_name.clone(),
                        tokio::sync::RwLock::new(client),
                    )])))
                    .map_err(|_| "failed to set MCP_SERVICES".to_string())?;
            }
        }

        // Register for lifecycle management
        let metadata = Arc::new(RwLock::new(ProcessMetadata::new(name, pid)));
        {
            let mut meta = metadata.write().await;
            meta.status = ProcessStatus::Running;
        }

        let info = Arc::new(StdioProcessInfo {
            metadata: Arc::clone(&metadata),
            service_name: service_name.clone(),
            fallback_message,
        });

        self.processes.write().await.insert(name.to_string(), info);

        // Start health monitoring
        self.health_monitor
            .start_monitoring(
                name.to_string(),
                metadata,
                service_name,
                config.stdio.clone(),
                config.clone(),
                Arc::clone(&self.recovery_manager),
            )
            .await;

        Ok(())
    }

    /// Stop a running process by name.
    ///
    /// Removes from MCP_SERVICES (dropping `RunningService` auto-kills the
    /// child process via `ChildWithCleanup` Drop impl) and stops health monitoring.
    pub async fn stop_process(&self, name: &str) -> Result<(), String> {
        let info = {
            let mut processes = self.processes.write().await;
            processes
                .remove(name)
                .ok_or_else(|| format!("process '{name}' not found"))?
        };

        // Stop health monitoring
        self.health_monitor.stop_monitoring(name).await;

        // Remove from MCP_SERVICES (dropping RunningService kills child process)
        if let Some(services) = MCP_SERVICES.get() {
            let mut services = services.write().await;
            if services.remove(&info.service_name).is_some() {
                dual_info!(
                    "[mcp-stdio:{}] Removed service '{}' from MCP_SERVICES",
                    name,
                    info.service_name
                );
            }
        }

        // Update status
        {
            let mut meta = info.metadata.write().await;
            meta.status = ProcessStatus::Stopped;
        }

        dual_info!("[mcp-stdio:{}] Process stopped", name);
        Ok(())
    }

    /// Restart a process with backoff delay.
    pub async fn restart_process(&self, name: &str) -> Result<(), String> {
        // Read restart info before stopping
        let (backoff_secs, restart_count) = {
            let processes = self.processes.read().await;
            let restart_count = if let Some(info) = processes.get(name) {
                let meta = info.metadata.read().await;
                meta.restart_count + 1
            } else {
                1
            };
            let configs = self.configs.read().await;
            let backoff = configs
                .get(name)
                .map(|c| c.stdio.restart_backoff_secs)
                .unwrap_or(5);
            (backoff, restart_count)
        };

        dual_info!(
            "[mcp-stdio:{}] Restarting process (attempt: {}, backoff: {}s)",
            name,
            restart_count,
            backoff_secs
        );

        // Stop current process
        if let Err(e) = self.stop_process(name).await {
            dual_warn!(
                "[mcp-stdio:{}] Error during stop before restart: {}",
                name,
                e
            );
        }

        // Backoff delay
        if backoff_secs > 0 {
            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
        }

        // Start again
        self.start_process(name).await?;

        // Restore restart count
        {
            let processes = self.processes.read().await;
            if let Some(info) = processes.get(name) {
                let mut meta = info.metadata.write().await;
                meta.restart_count = restart_count;
            }
        }

        Ok(())
    }

    /// Shutdown all managed processes.
    pub async fn shutdown_all(&self) {
        // Stop all health monitoring first
        self.health_monitor.stop_all().await;

        let names: Vec<String> = {
            let processes = self.processes.read().await;
            processes.keys().cloned().collect()
        };

        for name in names {
            if let Err(e) = self.stop_process(&name).await {
                dual_warn!("[mcp-stdio:{}] Error during shutdown: {}", name, e);
            }
        }

        dual_info!("[mcp-stdio] All processes shut down");
    }

    /// Background task that listens for restart requests from the health monitor.
    ///
    /// This decouples the health monitor from `StdioProcessManager` to avoid
    /// async opaque type cycles (`start_monitoring` → `restart_process` → `start_monitoring`).
    async fn restart_listener(mut restart_rx: mpsc::Receiver<String>) {
        while let Some(name) = restart_rx.recv().await {
            dual_info!(
                "[mcp-stdio:{}] Received restart request from health monitor",
                name
            );
            let manager = super::get_stdio_process_manager().clone();
            if let Err(e) = manager.restart_process(&name).await {
                dual_error!("[mcp-stdio:{}] Auto-restart failed: {}", name, e);
            }
        }
    }

    /// Get the status of a process.
    pub async fn get_process_status(&self, name: &str) -> Option<ProcessStatus> {
        let processes = self.processes.read().await;
        if let Some(info) = processes.get(name) {
            let meta = info.metadata.read().await;
            Some(meta.status.clone())
        } else {
            None
        }
    }

    /// List all managed processes with their metadata.
    pub async fn list_processes(&self) -> Vec<ProcessMetadata> {
        let processes = self.processes.read().await;
        let mut result = Vec::with_capacity(processes.len());
        for info in processes.values() {
            let meta = info.metadata.read().await;
            result.push(meta.clone());
        }
        result
    }
}

impl Default for StdioProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
