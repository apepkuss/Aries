//! Native Binary Executor
//!
//! Directly executes compiled binary files (no extension) as OS processes.
//! Used as a fallback for skill scripts that are native executables
//! rather than interpreted scripts (.js, .ts, .py, etc.).

use std::{process::Stdio, time::Instant};

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tracing::{debug, warn};

use super::{
    error::ExecutionError,
    traits::{Executor, IsolationLevel},
    types::{ExecuteRequest, ResourceUsage, ScriptOutput},
};

/// Executor for native binary files with no file extension.
///
/// Spawns the binary directly as an OS process without any interpreter.
/// Registered under the empty-string extension key `""` in the manager,
/// so it handles all scripts whose path has no extension.
#[derive(Default)]
pub struct NativeExecutor;

impl NativeExecutor {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Executor for NativeExecutor {
    fn name(&self) -> &str {
        "native"
    }

    /// Registers under `""` to handle extension-less binary files.
    fn supported_extensions(&self) -> Vec<&str> {
        vec![""]
    }

    fn isolation_level(&self) -> IsolationLevel {
        IsolationLevel::None
    }

    async fn execute(&self, request: ExecuteRequest) -> Result<ScriptOutput, ExecutionError> {
        let start_time = Instant::now();

        debug!(
            script = %request.script.path.display(),
            args = ?request.args,
            "executing native binary"
        );

        let mut cmd = tokio::process::Command::new(&request.script.path);
        cmd.args(&request.args)
            .envs(&request.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(working_dir) = &request.working_dir {
            cmd.current_dir(working_dir);
        } else if let Some(parent) = request.script.path.parent() {
            cmd.current_dir(parent);
        }

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ExecutionError::runtime(
                    "native",
                    format!("binary not found: '{}'", request.script.path.display()),
                )
            } else {
                ExecutionError::runtime(
                    "native",
                    format!("failed to spawn '{}': {e}", request.script.path.display()),
                )
            }
        })?;

        if let Some(stdin_data) = &request.stdin
            && let Some(mut stdin) = child.stdin.take()
        {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(stdin_data.as_bytes()).await {
                warn!(error = %e, "failed to write to stdin");
            }
        }

        let output_result = tokio::time::timeout(request.limits.timeout, async {
            let mut stdout_handle = child.stdout.take().unwrap();
            let mut stderr_handle = child.stderr.take().unwrap();

            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            let max_output = request.limits.max_output_bytes as usize;

            let (stdout_result, stderr_result, wait_result) = tokio::join!(
                async {
                    let mut buf = vec![0u8; 8192];
                    loop {
                        match stdout_handle.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                let remaining = max_output.saturating_sub(stdout.len());
                                if remaining > 0 {
                                    stdout.extend_from_slice(&buf[..n.min(remaining)]);
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "error reading stdout");
                                break;
                            }
                        }
                    }
                },
                async {
                    let mut buf = vec![0u8; 8192];
                    loop {
                        match stderr_handle.read(&mut buf).await {
                            Ok(0) => break,
                            Ok(n) => {
                                let remaining = max_output.saturating_sub(stderr.len());
                                if remaining > 0 {
                                    stderr.extend_from_slice(&buf[..n.min(remaining)]);
                                }
                            }
                            Err(e) => {
                                warn!(error = %e, "error reading stderr");
                                break;
                            }
                        }
                    }
                },
                child.wait()
            );

            let status = wait_result.map_err(|e| {
                ExecutionError::runtime("native", format!("failed to wait for process: {e}"))
            })?;

            let _ = stdout_result;
            let _ = stderr_result;

            Ok::<_, ExecutionError>((
                String::from_utf8_lossy(&stdout).to_string(),
                String::from_utf8_lossy(&stderr).to_string(),
                status.code().unwrap_or(-1),
            ))
        })
        .await;

        let duration = start_time.elapsed();

        match output_result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                debug!(
                    script = %request.script.path.display(),
                    exit_code,
                    duration = ?duration,
                    "native binary completed"
                );

                Ok(ScriptOutput {
                    stdout,
                    stderr,
                    exit_code,
                    duration,
                    resource_usage: ResourceUsage {
                        peak_memory_bytes: 0,
                        cpu_time_ms: duration.as_millis() as u64,
                    },
                    timed_out: false,
                })
            }
            Ok(Err(e)) => Err(e),
            Err(_) => {
                warn!(
                    script = %request.script.path.display(),
                    timeout = ?request.limits.timeout,
                    "native binary timed out"
                );

                Ok(ScriptOutput {
                    stdout: String::new(),
                    stderr: format!("execution timed out after {:?}", request.limits.timeout),
                    exit_code: -1,
                    duration,
                    resource_usage: ResourceUsage::default(),
                    timed_out: true,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, path::PathBuf};

    use super::*;
    use crate::{executor::ResourceLimits, skills::types::ScriptInfo};

    #[test]
    fn test_supported_extensions() {
        let executor = NativeExecutor::new();
        assert_eq!(executor.supported_extensions(), vec![""]);
    }

    #[test]
    fn test_isolation_level() {
        let executor = NativeExecutor::new();
        assert_eq!(executor.isolation_level(), IsolationLevel::None);
    }

    #[tokio::test]
    async fn test_execute_system_binary() {
        let executor = NativeExecutor::new();
        let script = ScriptInfo {
            name: "echo".to_string(),
            path: PathBuf::from("/bin/echo"),
            executable: true,
        };

        let request = ExecuteRequest {
            script,
            args: vec!["hello native".to_string()],
            env: HashMap::new(),
            limits: ResourceLimits::default(),
            working_dir: None,
            stdin: None,
        };

        let output = executor.execute(request).await.unwrap();
        assert_eq!(output.exit_code, 0);
        assert!(output.stdout.contains("hello native"));
    }
}
