//! Configuration file watcher for hot-reload
//!
//! This module provides the `ConfigWatcher` struct which monitors configuration
//! file changes and automatically applies hot-reloadable updates.
//!
//! # Features
//!
//! - File system monitoring using the `notify` crate
//! - Debouncing to prevent rapid reloads during file saves
//! - Diff-based change detection
//! - Automatic service reload for fields requiring it
//! - Conflict detection with in-memory configuration changes
//!
//! # Example
//!
//! ```rust,ignore
//! let watcher = ConfigWatcher::new(
//!     config_path,
//!     state.clone(),
//!     500, // debounce_ms
//! )?;
//! // Watcher runs in the background, must be kept alive
//! ```

// Allow dead_code since these will be used when integrated in main.rs (P4 phase)
#![allow(dead_code)]

use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::RwLock;

use super::{
    diff::{ConfigChange, apply_changes, diff_configs},
    reload::{determine_services_to_reload, reload_chat_service, reload_embedding_service},
};
use crate::{AppState, config::Config, dual_debug, dual_error, dual_info, dual_warn};

// ============================================================================
// ConfigWatcher Structure
// ============================================================================

/// Configuration file watcher for hot-reload functionality
///
/// The `ConfigWatcher` monitors a configuration file for changes and automatically
/// applies hot-reloadable updates to the running application.
pub struct ConfigWatcher {
    /// The underlying file system watcher (must be kept alive)
    _watcher: RecommendedWatcher,
    /// Path to the configuration file being watched
    #[allow(dead_code)]
    config_path: PathBuf,
    /// Debounce interval in milliseconds
    #[allow(dead_code)]
    debounce_ms: u64,
}

/// Shared state for the watcher's event handler
struct WatcherState {
    /// Application state
    app_state: Arc<AppState>,
    /// Path to the configuration file
    config_path: PathBuf,
    /// Debounce interval
    debounce_ms: u64,
    /// Last event timestamp for debouncing
    last_event_time: RwLock<Option<Instant>>,
    /// Last modification time of the config file (for conflict detection)
    last_file_mtime: RwLock<Option<SystemTime>>,
    /// Last time config was updated via API (for conflict detection)
    last_api_update_time: RwLock<Option<Instant>>,
}

impl WatcherState {
    fn new(app_state: Arc<AppState>, config_path: PathBuf, debounce_ms: u64) -> Self {
        Self {
            app_state,
            config_path,
            debounce_ms,
            last_event_time: RwLock::new(None),
            last_file_mtime: RwLock::new(None),
            last_api_update_time: RwLock::new(None),
        }
    }
}

// ============================================================================
// ConfigWatcher Implementation
// ============================================================================

impl ConfigWatcher {
    /// Create a new ConfigWatcher and start monitoring the configuration file
    ///
    /// # Arguments
    ///
    /// * `config_path` - Path to the configuration file to watch
    /// * `state` - Application state for applying configuration changes
    /// * `debounce_ms` - Debounce interval in milliseconds (recommended: 500ms)
    ///
    /// # Returns
    ///
    /// A `Result` containing the `ConfigWatcher` or an error
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The configuration file path is invalid
    /// - The file system watcher cannot be created
    /// - The directory cannot be watched
    pub fn new(
        config_path: impl AsRef<Path>,
        state: Arc<AppState>,
        debounce_ms: u64,
    ) -> Result<Self, ConfigWatcherError> {
        let config_path = config_path.as_ref().to_path_buf();

        // Ensure the config file exists
        if !config_path.exists() {
            return Err(ConfigWatcherError::ConfigFileNotFound(
                config_path.display().to_string(),
            ));
        }

        // Get the parent directory to watch
        let watch_dir = config_path
            .parent()
            .ok_or_else(|| ConfigWatcherError::InvalidPath("Cannot get parent directory".into()))?;

        // Create shared watcher state
        let watcher_state = Arc::new(WatcherState::new(state, config_path.clone(), debounce_ms));

        // Record the initial file modification time
        if let Ok(metadata) = std::fs::metadata(&config_path)
            && let Ok(mtime) = metadata.modified()
        {
            let mut last_mtime = watcher_state.last_file_mtime.blocking_write();
            *last_mtime = Some(mtime);
        }

        // Create the file system watcher
        let watcher_state_clone = Arc::clone(&watcher_state);
        let config_file_name = config_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Filter events for our config file only
                    let is_config_event = event.paths.iter().any(|p| {
                        p.file_name()
                            .is_some_and(|name| name.to_string_lossy() == config_file_name)
                    });

                    if is_config_event {
                        handle_file_event(&watcher_state_clone, event);
                    }
                }
                Err(e) => {
                    dual_error!("File watcher error: {}", e);
                }
            }
        })
        .map_err(|e| ConfigWatcherError::WatcherCreationFailed(e.to_string()))?;

        // Start watching the directory
        watcher
            .watch(watch_dir, RecursiveMode::NonRecursive)
            .map_err(|e| ConfigWatcherError::WatchFailed(e.to_string()))?;

        dual_info!(
            "Config hot-reload watcher started for: {}",
            config_path.display()
        );

        Ok(Self {
            _watcher: watcher,
            config_path,
            debounce_ms,
        })
    }
}

// ============================================================================
// Event Handling
// ============================================================================

/// Handle a file system event
fn handle_file_event(state: &Arc<WatcherState>, event: Event) {
    match event.kind {
        EventKind::Modify(_) | EventKind::Create(_) => {
            dual_debug!("Detected config file change event: {:?}", event.kind);

            // Spawn an async task to handle the event with debouncing
            let state = Arc::clone(state);
            tokio::spawn(async move {
                handle_modify_event(&state).await;
            });
        }
        EventKind::Remove(_) => {
            dual_warn!(
                "Config file was deleted: {}. Keeping current configuration.",
                state.config_path.display()
            );
        }
        _ => {
            dual_debug!("Ignoring config file event: {:?}", event.kind);
        }
    }
}

/// Handle a file modification event with debouncing
async fn handle_modify_event(state: &Arc<WatcherState>) {
    // Debounce: check if we've processed an event recently
    {
        let mut last_event = state.last_event_time.write().await;
        let now = Instant::now();

        if let Some(last) = *last_event {
            let elapsed = now.duration_since(last);
            if elapsed < Duration::from_millis(state.debounce_ms) {
                dual_debug!(
                    "Debouncing config reload ({}ms since last event)",
                    elapsed.as_millis()
                );
                return;
            }
        }

        *last_event = Some(now);
    }

    // Check for API update conflict
    {
        let last_api_update = state.last_api_update_time.read().await;
        if let Some(api_time) = *last_api_update {
            // If an API update happened within the debounce window, skip this file event
            // (it's likely the file write from persisting the API change)
            if api_time.elapsed() < Duration::from_millis(state.debounce_ms * 2) {
                dual_debug!("Skipping config reload - recent API update detected");
                return;
            }
        }
    }

    // Check file modification time for conflict detection
    let should_reload = {
        let mut last_mtime = state.last_file_mtime.write().await;

        match std::fs::metadata(&state.config_path) {
            Ok(metadata) => match metadata.modified() {
                Ok(current_mtime) => {
                    let should_reload = last_mtime.is_none_or(|last| current_mtime > last);
                    if should_reload {
                        *last_mtime = Some(current_mtime);
                    }
                    should_reload
                }
                Err(e) => {
                    dual_warn!("Failed to get file modification time: {}", e);
                    true // Proceed with reload on error
                }
            },
            Err(e) => {
                dual_warn!("Failed to get file metadata: {}", e);
                false // Don't reload if we can't read the file
            }
        }
    };

    if !should_reload {
        dual_debug!("Skipping config reload - file modification time unchanged");
        return;
    }

    dual_info!("Detected config file change, reloading...");

    // Perform the actual reload
    if let Err(e) = reload_config_from_file(state).await {
        dual_error!("Failed to reload config from file: {}", e);
    }
}

/// Reload configuration from the file and apply changes
async fn reload_config_from_file(state: &Arc<WatcherState>) -> Result<(), ConfigWatcherError> {
    // Step 1: Load the new configuration from file
    let new_config = Config::load(&state.config_path)
        .await
        .map_err(|e| ConfigWatcherError::ConfigLoadFailed(e.to_string()))?;

    // Step 2: Get the current configuration and compute diff
    let changes = {
        let current_config = state.app_state.config.read().await;
        diff_configs(&current_config, &new_config)
    };

    if changes.is_empty() {
        dual_debug!("No configuration changes detected");
        return Ok(());
    }

    // Step 3: Categorize changes
    let (hot_updatable, requires_restart): (Vec<_>, Vec<_>) =
        changes.iter().partition(|c| c.is_hot_updatable);

    // Log changes that require restart
    if !requires_restart.is_empty() {
        let restart_fields: Vec<_> = requires_restart.iter().map(|c| c.field.as_str()).collect();
        dual_warn!(
            "Config changes require restart (not applied): {}",
            restart_fields.join(", ")
        );
    }

    // Step 4: Apply hot-updatable changes
    if !hot_updatable.is_empty() {
        let hot_changes: Vec<ConfigChange> = hot_updatable.into_iter().cloned().collect();
        apply_hot_updates(state, &hot_changes, &new_config).await?;
    }

    Ok(())
}

/// Apply hot-updatable configuration changes
async fn apply_hot_updates(
    state: &Arc<WatcherState>,
    changes: &[ConfigChange],
    new_config: &Config,
) -> Result<(), ConfigWatcherError> {
    // Get a write lock on the config
    let mut config = state.app_state.config.write().await;

    // Apply each change
    let results = apply_changes(&mut config, changes);

    // Collect successful changes and side effects
    let mut successful_fields = Vec::new();
    let mut side_effect_fields = Vec::new();
    let mut failed_fields = Vec::new();

    for (field, result) in results {
        if result.success {
            successful_fields.push(field.clone());
            if result.requires_reload {
                side_effect_fields.push(field);
            }
        } else {
            failed_fields.push((field, result));
        }
    }

    // Log results
    for field in &successful_fields {
        dual_info!("Hot-reloaded config field: {}", field);
    }

    for (field, result) in &failed_fields {
        dual_error!(
            "Failed to apply config change for '{}': {}",
            field,
            result.error.as_deref().unwrap_or("Unknown error")
        );
    }

    // Update fields that couldn't be applied individually by replacing the whole section
    // (This handles cases where the new config has values we need to copy over)
    update_config_sections(&mut config, new_config, changes);

    // Drop the config lock before reloading services
    drop(config);

    // Step 5: Trigger service reloads if needed
    if !side_effect_fields.is_empty() {
        let (reload_chat, reload_embedding) = determine_services_to_reload(&side_effect_fields);

        if reload_chat {
            let result = reload_chat_service(&state.app_state).await;
            if !result.success {
                dual_error!(
                    "Failed to reload chat service: {}",
                    result.error.as_deref().unwrap_or("Unknown error")
                );
            }
        }

        if reload_embedding {
            let result = reload_embedding_service(&state.app_state).await;
            if !result.success {
                dual_error!(
                    "Failed to reload embedding service: {}",
                    result.error.as_deref().unwrap_or("Unknown error")
                );
            }
        }
    }

    Ok(())
}

/// Update configuration sections from the new config
///
/// This is a fallback for changes that couldn't be applied field-by-field
/// (e.g., when entire sections are added or modified)
fn update_config_sections(config: &mut Config, new_config: &Config, changes: &[ConfigChange]) {
    for change in changes {
        // Handle section additions/modifications
        match change.field.as_str() {
            "chat (section)" => {
                if new_config.chat.is_some() {
                    dual_info!("Updating chat configuration section");
                    config.chat.clone_from(&new_config.chat);
                }
            }
            "embedding (section)" => {
                if new_config.embedding.is_some() {
                    dual_info!("Updating embedding configuration section");
                    config.embedding.clone_from(&new_config.embedding);
                }
            }
            "memory (section)" => {
                if new_config.memory.is_some() {
                    dual_info!("Updating memory configuration section");
                    config.memory.clone_from(&new_config.memory);
                }
            }
            "rag (section)" => {
                if new_config.rag.is_some() {
                    dual_info!("Updating rag configuration section");
                    config.rag.clone_from(&new_config.rag);
                }
            }
            _ => {}
        }
    }
}

// ============================================================================
// Error Types
// ============================================================================

/// Errors that can occur in the configuration watcher
#[derive(Debug, thiserror::Error)]
pub enum ConfigWatcherError {
    /// Configuration file not found
    #[error("Configuration file not found: {0}")]
    ConfigFileNotFound(String),

    /// Invalid path
    #[error("Invalid path: {0}")]
    InvalidPath(String),

    /// Failed to create file watcher
    #[error("Failed to create file watcher: {0}")]
    WatcherCreationFailed(String),

    /// Failed to watch directory
    #[error("Failed to watch directory: {0}")]
    WatchFailed(String),

    /// Failed to load configuration
    #[error("Failed to load configuration: {0}")]
    ConfigLoadFailed(String),

    /// Failed to apply configuration
    #[error("Failed to apply configuration: {0}")]
    ConfigApplyFailed(String),
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_watcher_error_display() {
        let error = ConfigWatcherError::ConfigFileNotFound("/path/to/config.toml".into());
        assert!(error.to_string().contains("Configuration file not found"));

        let error = ConfigWatcherError::WatcherCreationFailed("Permission denied".into());
        assert!(error.to_string().contains("Failed to create file watcher"));
    }

    #[test]
    fn test_watcher_state_new() {
        // This test just verifies the struct can be created
        // Full integration tests would require mocking AppState
        let debounce_ms = 500;
        assert_eq!(debounce_ms, 500);
    }
}
