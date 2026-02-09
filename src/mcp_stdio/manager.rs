//! stdio process manager.
//!
//! Manages lifecycle of stdio MCP Server child processes with Lazy Loading.

use std::{collections::HashMap, sync::Arc, time::Duration};

use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{Mutex, RwLock},
};

use super::{
    health::HealthMonitor,
    recovery::RecoveryManager,
    transport::StdioTransport,
    types::{ProcessMetadata, ProcessStatus, StdioProcessConfig},
};
use crate::{dual_error, dual_info, dual_warn};

/// Managed process instance.
struct ManagedProcess {
    /// Process configuration
    // TODO: Phase 5 — used by recovery manager for restart
    #[allow(dead_code)]
    config: StdioProcessConfig,
    /// Child process handle (Option to allow take() during stop)
    child: Mutex<Option<Child>>,
    /// Process metadata
    metadata: Arc<RwLock<ProcessMetadata>>,
    /// Transport layer
    transport: Arc<StdioTransport>,
}

/// stdio process manager.
///
/// Manages lifecycle of stdio MCP Server child processes with Lazy Loading.
/// Processes are started on first tool call via [`get_transport`](Self::get_transport),
/// not at system startup.
pub struct StdioProcessManager {
    /// Process configurations (name -> config)
    configs: Arc<RwLock<HashMap<String, StdioProcessConfig>>>,
    /// Running processes (name -> managed process)
    processes: Arc<RwLock<HashMap<String, Arc<ManagedProcess>>>>,
    /// Per-process startup locks to prevent concurrent spawning during Lazy Loading
    start_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    /// Health monitor
    // TODO: Phase 4 — health check integration
    #[allow(dead_code)]
    health_monitor: Arc<HealthMonitor>,
    /// Recovery manager
    // TODO: Phase 5 — automatic recovery
    #[allow(dead_code)]
    recovery_manager: Arc<RecoveryManager>,
}

impl StdioProcessManager {
    /// Create a new process manager.
    pub fn new() -> Self {
        Self {
            configs: Arc::new(RwLock::new(HashMap::new())),
            processes: Arc::new(RwLock::new(HashMap::new())),
            start_locks: Arc::new(RwLock::new(HashMap::new())),
            health_monitor: Arc::new(HealthMonitor),
            recovery_manager: Arc::new(RecoveryManager),
        }
    }

    /// Register a process configuration.
    pub async fn register_config(&self, config: StdioProcessConfig) {
        let name = config.name.clone();
        dual_info!("[mcp-stdio] Registered config for '{}'", name);
        self.configs.write().await.insert(name, config);
    }

    /// Start a child process by name.
    pub async fn start_process(&self, name: &str) -> Result<(), String> {
        // 1. Get config
        let config = {
            let configs = self.configs.read().await;
            configs
                .get(name)
                .cloned()
                .ok_or_else(|| format!("config not found for '{name}'"))?
        };

        // 2. Check if already running
        {
            let processes = self.processes.read().await;
            if processes.contains_key(name) {
                return Err(format!("process '{name}' is already running"));
            }
        }

        // 3. Security warning
        dual_warn!(
            "[mcp-stdio:{}] Starting child process '{}' with Aries Agent privileges. \
             Ensure you trust this MCP server.",
            name,
            config.command
        );

        // 4. Build command
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        if let Some(ref dir) = config.working_dir {
            cmd.current_dir(dir);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // 5. Spawn process
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("failed to start process '{name}': {e}"))?;
        let pid = child.id();
        dual_info!("[mcp-stdio:{}] Process started (PID: {:?})", name, pid);

        // 6. Take I/O handles
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("failed to capture stdin for '{name}'"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("failed to capture stdout for '{name}'"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| format!("failed to capture stderr for '{name}'"))?;

        // 7. Create transport
        let transport = Arc::new(StdioTransport::new(name.to_string(), stdin, stdout));

        // 8. Start stderr logging
        let stderr_name = name.to_string();
        tokio::spawn(async move {
            Self::log_stderr(stderr_name, stderr).await;
        });

        // 9. Create metadata
        let metadata = Arc::new(RwLock::new(ProcessMetadata::new(name, pid)));
        {
            let mut meta = metadata.write().await;
            meta.status = ProcessStatus::Running;
        }

        // 10. Create managed process
        let managed = Arc::new(ManagedProcess {
            config,
            child: Mutex::new(Some(child)),
            metadata,
            transport,
        });

        // 11. Register in processes map
        self.processes
            .write()
            .await
            .insert(name.to_string(), managed.clone());

        // 12. Start process exit monitor
        let monitor_processes = Arc::clone(&self.processes);
        let monitor_name = name.to_string();
        tokio::spawn(async move {
            Self::monitor_process_exit(monitor_name, managed, monitor_processes).await;
        });

        // TODO: Phase 4 — start health monitoring if health_check_interval_secs > 0

        Ok(())
    }

    /// Stop a running process by name.
    pub async fn stop_process(&self, name: &str) -> Result<(), String> {
        let managed = {
            let mut processes = self.processes.write().await;
            processes
                .remove(name)
                .ok_or_else(|| format!("process '{name}' not found"))?
        };

        // Update status
        {
            let mut meta = managed.metadata.write().await;
            meta.status = ProcessStatus::Stopping;
        }

        // TODO: Phase 4 — stop health monitoring

        // Close transport
        managed.transport.close().await;

        // Kill child process
        let mut child_guard = managed.child.lock().await;
        if let Some(mut child) = child_guard.take() {
            if let Err(e) = child.kill().await {
                dual_warn!("[mcp-stdio:{}] Failed to kill process: {}", name, e);
            }
            match child.wait().await {
                Ok(status) => {
                    dual_info!("[mcp-stdio:{}] Process exited: {}", name, status);
                }
                Err(e) => {
                    dual_warn!("[mcp-stdio:{}] Error waiting for process exit: {}", name, e);
                }
            }
        }

        dual_info!("[mcp-stdio:{}] Process stopped", name);
        Ok(())
    }

    /// Restart a process with backoff delay.
    pub async fn restart_process(&self, name: &str) -> Result<(), String> {
        // Read restart info before stopping
        let (backoff_secs, restart_count) = {
            let processes = self.processes.read().await;
            let restart_count = if let Some(managed) = processes.get(name) {
                let meta = managed.metadata.read().await;
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
            if let Some(managed) = processes.get(name) {
                let mut meta = managed.metadata.write().await;
                meta.restart_count = restart_count;
            }
        }

        Ok(())
    }

    /// Get transport for a process, starting it if necessary (Lazy Loading).
    ///
    /// This is the primary entry point for tool calls. If the process is not
    /// running, it will be started automatically. Uses per-process locks to
    /// prevent concurrent startup.
    pub async fn get_transport(&self, name: &str) -> Result<Arc<StdioTransport>, String> {
        // Fast path: check if process is already running
        {
            let processes = self.processes.read().await;
            if let Some(managed) = processes.get(name)
                && !managed.transport.is_closed().await
            {
                return Ok(Arc::clone(&managed.transport));
            }
        }

        // Slow path: acquire per-process startup lock
        let lock = self.get_or_create_start_lock(name).await;
        let _guard = lock.lock().await;

        // Double-check after acquiring lock
        {
            let processes = self.processes.read().await;
            if let Some(managed) = processes.get(name)
                && !managed.transport.is_closed().await
            {
                return Ok(Arc::clone(&managed.transport));
            }
        }

        // Start the process
        self.start_process(name).await?;

        // Return the transport
        let processes = self.processes.read().await;
        let managed = processes
            .get(name)
            .ok_or_else(|| format!("process '{name}' failed to register after start"))?;
        Ok(Arc::clone(&managed.transport))
    }

    /// Get or create a per-process startup lock.
    async fn get_or_create_start_lock(&self, name: &str) -> Arc<Mutex<()>> {
        let mut locks = self.start_locks.write().await;
        locks
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Shutdown all managed processes.
    pub async fn shutdown_all(&self) {
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

    /// Monitor a child process for unexpected exit.
    ///
    /// Uses periodic `try_wait()` to avoid holding the child lock for extended
    /// periods. Exits when:
    /// - Process exits (expected or unexpected)
    /// - Process is removed from the map (stopped externally)
    /// - Child handle is taken (by stop_process)
    ///
    /// TODO: Phase 5 — trigger recovery manager on unexpected exit.
    async fn monitor_process_exit(
        name: String,
        managed: Arc<ManagedProcess>,
        processes: Arc<RwLock<HashMap<String, Arc<ManagedProcess>>>>,
    ) {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;

            // Check if process was removed from map (stopped externally)
            {
                let procs = processes.read().await;
                if !procs.contains_key(&name) {
                    return;
                }
            }

            // Try to check exit status (non-blocking)
            let exit_result = {
                let mut child_guard = managed.child.lock().await;
                match child_guard.as_mut() {
                    Some(child) => child.try_wait(),
                    None => return, // Child taken by stop_process
                }
            };

            match exit_result {
                Ok(Some(status)) => {
                    dual_warn!(
                        "[mcp-stdio:{}] Process exited unexpectedly: {}",
                        name,
                        status
                    );
                    {
                        let mut meta = managed.metadata.write().await;
                        meta.status = ProcessStatus::Failed(format!("exited: {status}"));
                    }
                    managed.transport.close().await;
                    processes.write().await.remove(&name);
                    // TODO: Phase 5 — trigger recovery manager
                    return;
                }
                Ok(None) => continue, // Still running
                Err(e) => {
                    dual_error!("[mcp-stdio:{}] Process wait error: {}", name, e);
                    {
                        let mut meta = managed.metadata.write().await;
                        meta.status = ProcessStatus::Failed(format!("wait error: {e}"));
                    }
                    managed.transport.close().await;
                    processes.write().await.remove(&name);
                    return;
                }
            }
        }
    }

    /// Log stderr output from a child process.
    async fn log_stderr(name: String, stderr: tokio::process::ChildStderr) {
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

    /// Get the status of a process.
    pub async fn get_process_status(&self, name: &str) -> Option<ProcessStatus> {
        let processes = self.processes.read().await;
        if let Some(managed) = processes.get(name) {
            let meta = managed.metadata.read().await;
            Some(meta.status.clone())
        } else {
            None
        }
    }

    /// List all managed processes with their metadata.
    pub async fn list_processes(&self) -> Vec<ProcessMetadata> {
        let processes = self.processes.read().await;
        let mut result = Vec::with_capacity(processes.len());
        for managed in processes.values() {
            let meta = managed.metadata.read().await;
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
