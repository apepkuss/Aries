//! Wasmtime Executor
//!
//! Executes WebAssembly modules and components using the Wasmtime runtime.
//! Provides the strongest sandboxing among all executors — WASM modules run
//! in isolated linear memory with no host access unless explicitly granted.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::RwLock,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};
use wasmtime::{Engine, Module, Store, StoreLimits, StoreLimitsBuilder, component::Component};
use wasmtime_wasi::{
    DirPerms, FilePerms, I32Exit, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView,
    p2::pipe::MemoryInputPipe,
};

use super::{
    error::ExecutionError,
    traits::{Executor, IsolationLevel},
    types::{ExecuteRequest, FilesystemPolicy, ScriptOutput},
};
#[cfg(test)]
use crate::skills::types::ScriptInfo;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Wasmtime executor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmtimeConfig {
    /// Enable or disable the wasmtime executor (default: true)
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Enable module compilation cache (default: true)
    #[serde(default = "default_cache_enabled")]
    pub cache_enabled: bool,

    /// Maximum WASM linear memory in bytes (default: 256 MB)
    #[serde(default = "default_max_memory_bytes")]
    pub max_memory_bytes: u64,

    /// Epoch tick interval in milliseconds for timeout checking (default: 10)
    #[serde(default = "default_epoch_tick_ms")]
    pub epoch_tick_ms: u64,

    /// Enable fuel-based CPU instruction limiting (default: false)
    #[serde(default)]
    pub fuel_enabled: bool,

    /// Amount of fuel to provide (~instructions, default: 1 billion)
    #[serde(default = "default_fuel_amount")]
    pub fuel_amount: u64,
}

fn default_enabled() -> bool {
    true
}
fn default_cache_enabled() -> bool {
    true
}
fn default_max_memory_bytes() -> u64 {
    256 * 1024 * 1024
}
fn default_epoch_tick_ms() -> u64 {
    10
}
fn default_fuel_amount() -> u64 {
    1_000_000_000
}

impl Default for WasmtimeConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            cache_enabled: default_cache_enabled(),
            max_memory_bytes: default_max_memory_bytes(),
            epoch_tick_ms: default_epoch_tick_ms(),
            fuel_enabled: false,
            fuel_amount: default_fuel_amount(),
        }
    }
}

// ---------------------------------------------------------------------------
// Cached module representation
// ---------------------------------------------------------------------------

/// A pre-compiled module or component kept in the cache.
#[allow(dead_code)]
enum CachedModule {
    Core(Module),
    Component(Component),
}

// ---------------------------------------------------------------------------
// Per-invocation store states
// ---------------------------------------------------------------------------

/// State carried in `Store<WasmP1State>` for WASI preview-1 (core) modules.
struct WasmP1State {
    wasi_p1: wasmtime_wasi::p1::WasiP1Ctx,
    limits: StoreLimits,
}

/// State carried in `Store<WasmP2State>` for WASI preview-2 (component) modules.
struct WasmP2State {
    wasi_ctx: WasiCtx,
    resource_table: wasmtime::component::ResourceTable,
    limits: StoreLimits,
}

impl WasiView for WasmP2State {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi_ctx,
            table: &mut self.resource_table,
        }
    }
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// Wasmtime-based WebAssembly executor.
pub struct WasmtimeExecutor {
    engine: Engine,
    config: WasmtimeConfig,
    module_cache: RwLock<HashMap<PathBuf, CachedModule>>,
}

impl WasmtimeExecutor {
    /// Create a new executor with the given configuration.
    pub fn with_config(config: WasmtimeConfig) -> Result<Self, ExecutionError> {
        let mut engine_config = wasmtime::Config::new();
        engine_config.async_support(true);
        engine_config.epoch_interruption(true);

        if config.fuel_enabled {
            engine_config.consume_fuel(true);
        }

        let engine = Engine::new(&engine_config).map_err(|e| {
            ExecutionError::ConfigError(format!("failed to create wasmtime engine: {e}"))
        })?;

        Ok(Self {
            engine,
            config,
            module_cache: RwLock::new(HashMap::new()),
        })
    }

    // ------------------------------------------------------------------
    // Module loading helpers
    // ------------------------------------------------------------------

    /// Load (and optionally cache) a module or component from `path`.
    fn load_module(&self, path: &PathBuf) -> Result<bool, ExecutionError> {
        // Already cached?
        if self.config.cache_enabled {
            let cache = self.module_cache.read().map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("cache lock poisoned: {e}"))
            })?;
            if cache.contains_key(path) {
                return Ok(true); // cache hit
            }
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let cached = if ext == "wat" {
            // WAT is always a core module
            let module = Module::from_file(&self.engine, path).map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("failed to compile WAT: {e}"))
            })?;
            CachedModule::Core(module)
        } else {
            // .wasm — try Component first, fall back to core Module
            match Component::from_file(&self.engine, path) {
                Ok(component) => CachedModule::Component(component),
                Err(_) => {
                    let module = Module::from_file(&self.engine, path).map_err(|e| {
                        ExecutionError::runtime(
                            "wasmtime",
                            format!("failed to compile WASM module: {e}"),
                        )
                    })?;
                    CachedModule::Core(module)
                }
            }
        };

        if self.config.cache_enabled {
            let mut cache = self.module_cache.write().map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("cache lock poisoned: {e}"))
            })?;
            cache.insert(path.clone(), cached);
        }

        Ok(false) // cache miss
    }

    /// Returns `true` if the cached entry for `path` is a Component.
    fn is_component(&self, path: &PathBuf) -> bool {
        if let Ok(cache) = self.module_cache.read() {
            matches!(cache.get(path), Some(CachedModule::Component(_)))
        } else {
            false
        }
    }

    // ------------------------------------------------------------------
    // WASI builder helpers
    // ------------------------------------------------------------------

    fn build_wasi_builder(&self, request: &ExecuteRequest) -> WasiCtxBuilder {
        let mut builder = WasiCtx::builder();

        // Inherit stdio via pipes – we capture stdout/stderr later
        builder.inherit_stdout();
        builder.inherit_stderr();

        // Args: program name + user args
        builder.arg(&request.script.name);
        for arg in &request.args {
            builder.arg(arg);
        }

        // Environment
        for (k, v) in &request.env {
            builder.env(k, v);
        }

        // stdin
        if let Some(ref input) = request.stdin {
            let pipe = MemoryInputPipe::new(bytes::Bytes::from(input.clone()));
            builder.stdin(pipe);
        }

        // Filesystem pre-opens
        match &request.limits.filesystem_access {
            FilesystemPolicy::ReadOnly(paths) => {
                for p in paths {
                    if let Err(e) = builder.preopened_dir(
                        p,
                        p.to_string_lossy().as_ref(),
                        DirPerms::READ,
                        FilePerms::READ,
                    ) {
                        warn!(path = %p.display(), error = %e, "failed to preopen dir (ro)");
                    }
                }
            }
            FilesystemPolicy::ReadWrite(paths) => {
                for p in paths {
                    if let Err(e) = builder.preopened_dir(
                        p,
                        p.to_string_lossy().as_ref(),
                        DirPerms::all(),
                        FilePerms::all(),
                    ) {
                        warn!(path = %p.display(), error = %e, "failed to preopen dir (rw)");
                    }
                }
            }
            FilesystemPolicy::None => {}
        }

        builder
    }

    fn build_store_limits(&self, request: &ExecuteRequest) -> StoreLimits {
        StoreLimitsBuilder::new()
            .memory_size(request.limits.max_memory_bytes as usize)
            .build()
    }

    // ------------------------------------------------------------------
    // Epoch ticker
    // ------------------------------------------------------------------

    /// Start an OS-thread epoch ticker. Returns a sender that stops the
    /// ticker when dropped or when `send(())` is called.
    fn start_epoch_ticker(&self) -> std::sync::mpsc::Sender<()> {
        let engine = self.engine.clone();
        let tick_ms = self.config.epoch_tick_ms;
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            loop {
                match stop_rx.recv_timeout(Duration::from_millis(tick_ms)) {
                    Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        engine.increment_epoch();
                    }
                }
            }
        });
        stop_tx
    }

    // ------------------------------------------------------------------
    // Execution paths
    // ------------------------------------------------------------------

    /// Execute a WASI preview-1 (core) module.
    async fn execute_p1(
        &self,
        path: &PathBuf,
        request: &ExecuteRequest,
    ) -> Result<ScriptOutput, ExecutionError> {
        let start = Instant::now();

        let mut wasi_builder = self.build_wasi_builder(request);
        let wasi_p1 = wasi_builder.build_p1();
        let limits = self.build_store_limits(request);

        let state = WasmP1State { wasi_p1, limits };

        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);

        // Configure epoch-based timeout: yield on each tick to allow the
        // async runtime to run the epoch ticker, then interrupt after timeout.
        let timeout = request.limits.timeout;
        let deadline_start = Instant::now();
        store.epoch_deadline_callback(move |_ctx| {
            if deadline_start.elapsed() >= timeout {
                Ok(wasmtime::UpdateDeadline::Interrupt)
            } else {
                Ok(wasmtime::UpdateDeadline::Yield(1))
            }
        });
        store.set_epoch_deadline(1);

        if self.config.fuel_enabled {
            let _ = store.set_fuel(self.config.fuel_amount);
        }

        let ticker = self.start_epoch_ticker();

        // Build linker
        let mut linker = wasmtime::Linker::new(&self.engine);
        linker.allow_shadowing(true);
        wasmtime_wasi::p1::add_to_linker_async(&mut linker, |state: &mut WasmP1State| {
            &mut state.wasi_p1
        })
        .map_err(|e| ExecutionError::runtime("wasmtime", format!("failed to link WASI p1: {e}")))?;

        // Get module from cache (scope ensures guard is dropped before await)
        let module = {
            let cache = self.module_cache.read().map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("cache lock poisoned: {e}"))
            })?;
            match cache.get(path) {
                Some(CachedModule::Core(m)) => m.clone(),
                _ => {
                    return Err(ExecutionError::runtime(
                        "wasmtime",
                        "module not found in cache",
                    ));
                }
            }
        };

        // Instantiate and run _start
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("instantiation failed: {e}"))
            })?;

        let start_func = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .map_err(|e| ExecutionError::runtime("wasmtime", format!("_start not found: {e}")))?;

        let result = start_func.call_async(&mut store, ()).await;
        drop(ticker);

        let duration = start.elapsed();

        match result {
            Ok(()) => Ok(ScriptOutput {
                exit_code: 0,
                duration,
                ..Default::default()
            }),
            Err(err) => Self::handle_trap(err, duration),
        }
    }

    /// Execute a WASI preview-2 (component model) module.
    async fn execute_p2(
        &self,
        path: &PathBuf,
        request: &ExecuteRequest,
    ) -> Result<ScriptOutput, ExecutionError> {
        let start = Instant::now();

        let mut wasi_builder = self.build_wasi_builder(request);
        let wasi_ctx = wasi_builder.build();
        let limits = self.build_store_limits(request);

        let state = WasmP2State {
            wasi_ctx,
            resource_table: wasmtime::component::ResourceTable::new(),
            limits,
        };

        let mut store = Store::new(&self.engine, state);
        store.limiter(|s| &mut s.limits);

        let timeout = request.limits.timeout;
        let deadline_start = Instant::now();
        store.epoch_deadline_callback(move |_ctx| {
            if deadline_start.elapsed() >= timeout {
                Ok(wasmtime::UpdateDeadline::Interrupt)
            } else {
                Ok(wasmtime::UpdateDeadline::Yield(1))
            }
        });
        store.set_epoch_deadline(1);

        if self.config.fuel_enabled {
            let _ = store.set_fuel(self.config.fuel_amount);
        }

        let ticker = self.start_epoch_ticker();

        // Build component linker
        let mut linker = wasmtime::component::Linker::new(&self.engine);
        wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| {
            ExecutionError::runtime("wasmtime", format!("failed to link WASI p2: {e}"))
        })?;

        // Get component from cache (scope ensures guard is dropped before await)
        let component = {
            let cache = self.module_cache.read().map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("cache lock poisoned: {e}"))
            })?;
            match cache.get(path) {
                Some(CachedModule::Component(c)) => c.clone(),
                _ => {
                    return Err(ExecutionError::runtime(
                        "wasmtime",
                        "component not found in cache",
                    ));
                }
            }
        };

        // Use CommandPre to instantiate and run
        let command_pre = wasmtime_wasi::p2::bindings::CommandPre::new(
            linker.instantiate_pre(&component).map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("pre-instantiation failed: {e}"))
            })?,
        )
        .map_err(|e| {
            ExecutionError::runtime("wasmtime", format!("CommandPre creation failed: {e}"))
        })?;

        let command = command_pre
            .instantiate_async(&mut store)
            .await
            .map_err(|e| {
                ExecutionError::runtime("wasmtime", format!("instantiation failed: {e}"))
            })?;

        let result = command.wasi_cli_run().call_run(&mut store).await;

        drop(ticker);
        let duration = start.elapsed();

        match result {
            Ok(Ok(())) => Ok(ScriptOutput {
                exit_code: 0,
                duration,
                ..Default::default()
            }),
            Ok(Err(())) => Ok(ScriptOutput {
                exit_code: 1,
                duration,
                ..Default::default()
            }),
            Err(err) => Self::handle_trap(err, duration),
        }
    }

    // ------------------------------------------------------------------
    // Error handling
    // ------------------------------------------------------------------

    fn handle_trap(
        err: wasmtime::Error,
        duration: Duration,
    ) -> Result<ScriptOutput, ExecutionError> {
        // Check for I32Exit (normal process exit with code)
        if let Some(exit) = err.downcast_ref::<I32Exit>() {
            return Ok(ScriptOutput {
                exit_code: exit.0,
                duration,
                ..Default::default()
            });
        }

        // Check for epoch interruption (timeout)
        let trap = err.downcast_ref::<wasmtime::Trap>();
        if trap == Some(&wasmtime::Trap::Interrupt) {
            return Ok(ScriptOutput {
                exit_code: -1,
                duration,
                timed_out: true,
                stderr: "execution timed out".to_string(),
                ..Default::default()
            });
        }

        Err(ExecutionError::runtime(
            "wasmtime",
            format!("execution failed: {err}"),
        ))
    }
}

// ---------------------------------------------------------------------------
// Executor trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl Executor for WasmtimeExecutor {
    fn name(&self) -> &str {
        "wasmtime"
    }

    fn supported_extensions(&self) -> Vec<&str> {
        vec!["wasm", "wat"]
    }

    fn isolation_level(&self) -> IsolationLevel {
        IsolationLevel::Runtime
    }

    async fn health_check(&self) -> Result<(), ExecutionError> {
        // Verify engine can compile a trivial module
        let wat = "(module)";
        Module::new(&self.engine, wat).map_err(|e| {
            ExecutionError::runtime("wasmtime", format!("health check failed: {e}"))
        })?;
        Ok(())
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ScriptOutput, ExecutionError> {
        let path = &request.script.path;

        // Check file existence
        if !path.exists() {
            return Err(ExecutionError::ScriptNotFound(path.clone()));
        }

        debug!(
            script = %path.display(),
            "wasmtime: loading module"
        );

        // Load / compile
        let _cache_hit = self.load_module(path)?;

        // Dispatch to P1 or P2 path
        if self.is_component(path) {
            self.execute_p2(path, &request).await
        } else {
            self.execute_p1(path, &request).await
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::types::ResourceLimits;

    fn project_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    fn fixture(name: &str) -> PathBuf {
        project_root().join("tests/fixtures/wasm").join(name)
    }

    // ---------------------------------------------------------------
    // Unit tests
    // ---------------------------------------------------------------

    #[test]
    fn test_wasmtime_config_default() {
        let cfg = WasmtimeConfig::default();
        assert!(cfg.enabled);
        assert!(cfg.cache_enabled);
        assert_eq!(cfg.max_memory_bytes, 256 * 1024 * 1024);
        assert_eq!(cfg.epoch_tick_ms, 10);
        assert!(!cfg.fuel_enabled);
        assert_eq!(cfg.fuel_amount, 1_000_000_000);
    }

    #[test]
    fn test_executor_creation() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default());
        assert!(executor.is_ok());
    }

    #[test]
    fn test_supported_extensions() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        let exts = executor.supported_extensions();
        assert!(exts.contains(&"wasm"));
        assert!(exts.contains(&"wat"));
        assert_eq!(exts.len(), 2);
    }

    // ---------------------------------------------------------------
    // Async / integration tests
    // ---------------------------------------------------------------

    #[tokio::test]
    async fn test_health_check() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        assert!(executor.health_check().await.is_ok());
    }

    #[tokio::test]
    async fn test_execute_hello_wat() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        let script = ScriptInfo {
            name: "hello.wat".to_string(),
            path: fixture("hello.wat"),
            executable: true,
        };
        let request = ExecuteRequest {
            script,
            args: vec![],
            env: HashMap::new(),
            limits: ResourceLimits::default(),
            working_dir: None,
            stdin: None,
        };
        let output = executor.execute(request).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(!output.timed_out);
    }

    #[tokio::test]
    async fn test_execute_timeout() {
        let config = WasmtimeConfig {
            epoch_tick_ms: 5,
            ..Default::default()
        };
        let executor = WasmtimeExecutor::with_config(config).unwrap();
        let script = ScriptInfo {
            name: "infinite_loop.wat".to_string(),
            path: fixture("infinite_loop.wat"),
            executable: true,
        };
        let mut limits = ResourceLimits::default();
        limits.timeout = Duration::from_millis(100);
        let request = ExecuteRequest {
            script,
            args: vec![],
            env: HashMap::new(),
            limits,
            working_dir: None,
            stdin: None,
        };
        let output = executor.execute(request).await.unwrap();
        assert!(output.timed_out);
    }

    #[tokio::test]
    async fn test_execute_exit_code() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        let script = ScriptInfo {
            name: "exit_code.wat".to_string(),
            path: fixture("exit_code.wat"),
            executable: true,
        };
        let request = ExecuteRequest {
            script,
            args: vec![],
            env: HashMap::new(),
            limits: ResourceLimits::default(),
            working_dir: None,
            stdin: None,
        };
        let output = executor.execute(request).await.unwrap();
        assert_eq!(output.exit_code, 42);
    }

    #[tokio::test]
    async fn test_execute_not_found() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        let script = ScriptInfo {
            name: "nonexistent.wasm".to_string(),
            path: PathBuf::from("/tmp/nonexistent_wasmtime_test.wasm"),
            executable: true,
        };
        let request = ExecuteRequest {
            script,
            args: vec![],
            env: HashMap::new(),
            limits: ResourceLimits::default(),
            working_dir: None,
            stdin: None,
        };
        let result = executor.execute(request).await;
        assert!(matches!(result, Err(ExecutionError::ScriptNotFound(_))));
    }

    #[tokio::test]
    async fn test_execute_invalid_module() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();

        // Write invalid bytes to a temp file
        let tmp = tempfile::Builder::new().suffix(".wasm").tempfile().unwrap();
        std::fs::write(tmp.path(), b"not a valid wasm module").unwrap();

        let script = ScriptInfo {
            name: "invalid.wasm".to_string(),
            path: tmp.path().to_path_buf(),
            executable: true,
        };
        let request = ExecuteRequest {
            script,
            args: vec![],
            env: HashMap::new(),
            limits: ResourceLimits::default(),
            working_dir: None,
            stdin: None,
        };
        let result = executor.execute(request).await;
        assert!(matches!(result, Err(ExecutionError::RuntimeError { .. })));
    }

    #[tokio::test]
    async fn test_module_cache() {
        let executor = WasmtimeExecutor::with_config(WasmtimeConfig::default()).unwrap();
        let path = fixture("hello.wat");

        // First load → cache miss
        let hit = executor.load_module(&path).unwrap();
        assert!(!hit);

        // Second load → cache hit
        let hit = executor.load_module(&path).unwrap();
        assert!(hit);
    }
}
