//! Configuration persistence logic
//!
//! This module provides functions for persisting configuration changes to disk.
//! Configuration is saved atomically using a write-to-temp-then-rename strategy.

use std::path::{Path, PathBuf};

use tokio::{fs, io::AsyncWriteExt};

use crate::{config::Config, dual_error, dual_info, dual_warn};

/// Result of a configuration persistence operation
#[derive(Debug)]
pub struct PersistResult {
    /// Whether the persistence was successful
    pub success: bool,
    /// The path where config was saved
    #[allow(dead_code)]
    pub path: Option<PathBuf>,
    /// Error message if persistence failed
    pub error: Option<String>,
}

impl PersistResult {
    pub fn success(path: PathBuf) -> Self {
        Self {
            success: true,
            path: Some(path),
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            path: None,
            error: Some(error),
        }
    }
}

/// Persist configuration to a TOML file
///
/// This function:
/// 1. Serializes the config to TOML format
/// 2. Writes to a temporary file
/// 3. Atomically renames the temp file to the target path
///
/// # Arguments
///
/// * `config` - The configuration to persist
/// * `path` - The target file path
///
/// # Returns
///
/// A `PersistResult` indicating success or failure
pub async fn persist_config(config: &Config, path: &Path) -> PersistResult {
    dual_info!("Persisting configuration to: {}", path.display());

    // Step 1: Serialize config to TOML
    let toml_content = match toml::to_string_pretty(config) {
        Ok(content) => content,
        Err(e) => {
            let error_msg = format!("Failed to serialize config to TOML: {}", e);
            dual_error!("{}", error_msg);
            return PersistResult::failure(error_msg);
        }
    };

    // Step 2: Write to a temporary file first (atomic write strategy)
    let temp_path = path.with_extension("toml.tmp");

    if let Err(e) = write_file_atomic(&temp_path, &toml_content).await {
        let error_msg = format!("Failed to write temporary config file: {}", e);
        dual_error!("{}", error_msg);
        return PersistResult::failure(error_msg);
    }

    // Step 3: Atomically rename temp file to target path
    if let Err(e) = fs::rename(&temp_path, path).await {
        let error_msg = format!("Failed to rename config file: {}", e);
        dual_error!("{}", error_msg);

        // Try to clean up the temp file
        if let Err(cleanup_err) = fs::remove_file(&temp_path).await {
            dual_warn!(
                "Failed to clean up temporary file {}: {}",
                temp_path.display(),
                cleanup_err
            );
        }

        return PersistResult::failure(error_msg);
    }

    dual_info!(
        "Configuration persisted successfully to: {}",
        path.display()
    );
    PersistResult::success(path.to_path_buf())
}

/// Write content to a file with proper error handling
async fn write_file_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    // Write content
    let mut file = fs::File::create(path).await?;
    file.write_all(content.as_bytes()).await?;
    file.sync_all().await?;

    Ok(())
}

/// Create a backup of the current config file before overwriting
///
/// # Arguments
///
/// * `path` - The config file path to backup
///
/// # Returns
///
/// The backup file path if successful, None if backup failed or source doesn't exist
#[allow(dead_code)]
pub async fn backup_config(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }

    let backup_path = path.with_extension("toml.bak");

    match fs::copy(path, &backup_path).await {
        Ok(_) => {
            dual_info!("Config backup created: {}", backup_path.display());
            Some(backup_path)
        }
        Err(e) => {
            dual_warn!("Failed to create config backup: {}", e);
            None
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_persist_result_success() {
        let result = PersistResult::success(PathBuf::from("/tmp/config.toml"));
        assert!(result.success);
        assert!(result.path.is_some());
        assert!(result.error.is_none());
    }

    #[test]
    fn test_persist_result_failure() {
        let result = PersistResult::failure("Write error".to_string());
        assert!(!result.success);
        assert!(result.path.is_none());
        assert_eq!(result.error, Some("Write error".to_string()));
    }

    #[tokio::test]
    async fn test_persist_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("test_config.toml");

        let config = Config::default();
        let result = persist_config(&config, &config_path).await;

        assert!(result.success);
        assert!(config_path.exists());

        // Verify the content is valid TOML
        let content = fs::read_to_string(&config_path).await.unwrap();
        assert!(content.contains("[server]"));
    }

    #[tokio::test]
    async fn test_persist_config_creates_parent_dirs() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir
            .path()
            .join("subdir")
            .join("nested")
            .join("config.toml");

        let config = Config::default();
        let result = persist_config(&config, &config_path).await;

        assert!(result.success);
        assert!(config_path.exists());
    }

    #[tokio::test]
    async fn test_backup_config() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.toml");

        // Create initial config file
        fs::write(&config_path, "# test config").await.unwrap();

        let backup_path = backup_config(&config_path).await;
        assert!(backup_path.is_some());

        let backup = backup_path.unwrap();
        assert!(backup.exists());
        assert_eq!(backup.extension().unwrap(), "bak");
    }

    #[tokio::test]
    async fn test_backup_config_nonexistent_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("nonexistent.toml");

        let backup_path = backup_config(&config_path).await;
        assert!(backup_path.is_none());
    }
}
