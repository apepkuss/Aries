//! Skill dependency checking and auto-installation.
//!
//! Before a skill is activated, all binaries declared in `requires.bins` are
//! verified to exist in PATH.  Any missing binary is installed using the
//! first matching entry in the skill's `install` array, following the
//! OpenClaw install spec (brew / node / go / uv / download).
//!
//! Call [`SkillDependencyChecker::ensure_dependencies`] inside a
//! `tokio::task::spawn_blocking` block because all I/O here is synchronous.

use std::process::{Command, Stdio};

use tracing::info;

use crate::skills::types::SkillMetadata;

/// Checks and auto-installs binary dependencies declared by a skill.
pub struct SkillDependencyChecker;

impl SkillDependencyChecker {
    /// Returns `true` if `bin` is available in the current `PATH`.
    pub fn is_bin_available(bin: &str) -> bool {
        Command::new("which")
            .arg(bin)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// Returns the subset of `requires.bins` that are **not** currently in PATH.
    pub fn check_missing_bins(metadata: &SkillMetadata) -> Vec<String> {
        metadata
            .get_required_bins()
            .unwrap_or_default()
            .into_iter()
            .filter(|bin| !Self::is_bin_available(bin))
            .collect()
    }

    /// Returns `true` if the install option's `os` filter matches the current platform.
    /// An absent `os` field means "all platforms".
    fn matches_current_os(option: &serde_json::Value) -> bool {
        let current_os = if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "win32"
        } else {
            return false;
        };

        match option.get("os").and_then(|v| v.as_array()) {
            None => true,
            Some(list) => list.iter().any(|v| v.as_str() == Some(current_os)),
        }
    }

    /// Spawn `cmd args` with stdout/stderr inherited (real-time terminal output).
    fn run_command(cmd: &str, args: &[&str], label: &str) -> Result<(), String> {
        info!("[dependency] {} {}", cmd, args.join(" "));
        let status = Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|e| format!("failed to spawn '{}': {}", cmd, e))?;

        if status.success() {
            info!("[dependency] '{}' finished successfully", label);
            Ok(())
        } else {
            Err(format!("'{}' exited with code {:?}", label, status.code()))
        }
    }

    /// Handle `kind = "download"`:
    ///   - No archive → `curl -fsSL -o <targetDir>/<filename> <url>` + `chmod +x`
    ///   - tar.gz / tar.bz2 → `sh -c "curl … | tar -x…"`
    ///   - zip → `curl -o /tmp/…zip …` then `unzip`
    fn install_download(option: &serde_json::Value, label: &str) -> Result<(), String> {
        let url = option
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "download option missing 'url'".to_string())?;

        let target_dir = option
            .get("targetDir")
            .and_then(|v| v.as_str())
            .unwrap_or("~/.openclaw/tools");

        let archive = option.get("archive").and_then(|v| v.as_str());
        let strip = option
            .get("stripComponents")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // Expand leading `~/`
        let expanded_dir = if let Some(rest) = target_dir.strip_prefix("~/") {
            let home = std::env::var("HOME")
                .map_err(|_| "HOME environment variable not set".to_string())?;
            format!("{}/{}", home, rest)
        } else {
            target_dir.to_string()
        };

        std::fs::create_dir_all(&expanded_dir)
            .map_err(|e| format!("cannot create targetDir '{}': {}", expanded_dir, e))?;

        info!("[dependency] downloading '{}' from {}", label, url);

        match archive {
            None => {
                // Plain binary
                let filename = url.split('/').next_back().unwrap_or("binary");
                let dest = format!("{}/{}", expanded_dir, filename);
                Self::run_command("curl", &["-fsSL", "-o", &dest, url], label)?;
                Self::run_command("chmod", &["+x", &dest], label)?;
            }
            Some("zip") => {
                let tmp = "/tmp/_skill_dep_download.zip";
                Self::run_command("curl", &["-fsSL", "-o", tmp, url], label)?;
                Self::run_command("unzip", &["-o", tmp, "-d", &expanded_dir], label)?;
            }
            Some(fmt @ ("tar.gz" | "tar.bz2")) => {
                let flag = if fmt == "tar.gz" { "z" } else { "j" };
                let sh_cmd = format!(
                    "curl -fsSL '{}' | tar -x{} --strip-components {} -C '{}'",
                    url, flag, strip, expanded_dir
                );
                Self::run_command("sh", &["-c", &sh_cmd], label)?;
            }
            Some(other) => {
                return Err(format!("unknown archive format: '{}'", other));
            }
        }

        Ok(())
    }

    /// Attempt a single install option.
    ///
    /// Returns `Ok(())` on success or `Err(reason)` if the option is not
    /// applicable (OS mismatch, unknown kind, missing fields, or command failure).
    fn try_install(option: &serde_json::Value) -> Result<(), String> {
        if !Self::matches_current_os(option) {
            return Err("OS not matched, skipping".to_string());
        }

        let kind = option.get("kind").and_then(|v| v.as_str()).unwrap_or("");
        let label = option.get("label").and_then(|v| v.as_str()).unwrap_or(kind);

        info!(
            "[dependency] trying install option '{}' (kind={})",
            label, kind
        );

        match kind {
            "brew" => {
                let formula = option
                    .get("formula")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "brew option missing 'formula'".to_string())?;
                Self::run_command("brew", &["install", formula], label)
            }
            "node" => {
                let pkg = option
                    .get("package")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "node option missing 'package'".to_string())?;
                Self::run_command("npm", &["install", "-g", pkg], label)
            }
            "go" => {
                let pkg = option
                    .get("package")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "go option missing 'package'".to_string())?;
                let pkg_at_latest = format!("{}@latest", pkg);
                Self::run_command("go", &["install", &pkg_at_latest], label)
            }
            "uv" => {
                let pkg = option
                    .get("package")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "uv option missing 'package'".to_string())?;
                Self::run_command("uv", &["tool", "install", pkg], label)
            }
            "download" => Self::install_download(option, label),
            // Non-spec compat: pip / pip3
            "pip" | "pip3" => {
                let pkg = option
                    .get("package")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "pip option missing 'package'".to_string())?;
                Self::run_command("pip3", &["install", pkg], label)
            }
            other => Err(format!("unknown install kind: '{}'", other)),
        }
    }

    /// Iterate install options in declaration order, trying each one that
    /// provides at least one of the `missing` binaries.  Stops at first success.
    fn install_using_options(
        options: &[serde_json::Value],
        missing: &[String],
    ) -> Result<(), String> {
        let mut errors: Vec<String> = Vec::new();

        for option in options {
            // Only attempt options that claim to provide a missing binary
            let provides: Vec<&str> = option
                .get("bins")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();

            if !provides.iter().any(|b| missing.iter().any(|m| m == b)) {
                continue;
            }

            match Self::try_install(option) {
                Ok(()) => return Ok(()),
                Err(e) => errors.push(e),
            }
        }

        if errors.is_empty() {
            Err("no install option matched the missing binaries".to_string())
        } else {
            Err(errors.join("; "))
        }
    }

    /// Full dependency gate for a skill.
    ///
    /// 1. Collect required bins that are missing from PATH.
    /// 2. If none missing → return `Ok(())` immediately.
    /// 3. Try each matching install option until one succeeds.
    /// 4. Re-check PATH after installation.
    ///
    /// **Must be called inside `tokio::task::spawn_blocking`** — all I/O is
    /// synchronous (`std::process::Command`).
    pub fn ensure_dependencies(metadata: &SkillMetadata) -> Result<(), String> {
        let missing = Self::check_missing_bins(metadata);
        if missing.is_empty() {
            return Ok(());
        }

        let options = metadata.get_install_options();
        if options.is_empty() {
            return Err(format!(
                "skill '{}' requires [{}] but provides no install instructions",
                metadata.name,
                missing.join(", ")
            ));
        }

        info!(
            "[dependency] skill '{}': missing [{}], attempting auto-install",
            metadata.name,
            missing.join(", ")
        );

        Self::install_using_options(&options, &missing).map_err(|e| {
            format!(
                "skill '{}' requires [{}] — auto-install failed: {}",
                metadata.name,
                missing.join(", "),
                e
            )
        })?;

        // Verify binaries are now reachable
        let still_missing = Self::check_missing_bins(metadata);
        if still_missing.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "skill '{}': after install, [{}] still not found in PATH",
                metadata.name,
                still_missing.join(", ")
            ))
        }
    }
}
