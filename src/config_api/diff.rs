//! Configuration diff logic
//!
//! This module provides functions for comparing two configurations and
//! detecting changes. It identifies which fields have changed and whether
//! each change can be hot-reloaded or requires a restart.

// Allow dead_code since these functions will be used in P3 phase (file watcher)
#![allow(dead_code)]

use serde_json::json;

use super::types::UPDATABLE_FIELDS;
use crate::config::{
    ChatConfig, Config, EmbeddingConfig, MemoryConfig, RagConfig, ServerConfig,
    SummarizationStrategy,
};

// ============================================================================
// ConfigChange Structure
// ============================================================================

/// Represents a single configuration change
#[derive(Debug, Clone)]
pub struct ConfigChange {
    /// Field path (e.g., "server.max_tools_per_iteration")
    pub field: String,
    /// Value before the change
    pub old_value: serde_json::Value,
    /// Value after the change
    pub new_value: serde_json::Value,
    /// Whether this change can be hot-reloaded
    pub is_hot_updatable: bool,
}

impl ConfigChange {
    /// Create a new ConfigChange
    pub fn new(
        field: impl Into<String>,
        old_value: serde_json::Value,
        new_value: serde_json::Value,
        is_hot_updatable: bool,
    ) -> Self {
        Self {
            field: field.into(),
            old_value,
            new_value,
            is_hot_updatable,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if a field path is in the updatable fields list
pub fn is_updatable_field(field: &str) -> bool {
    UPDATABLE_FIELDS.contains(&field)
}

// ============================================================================
// Server Config Diff
// ============================================================================

/// Compare two ServerConfig instances and return a list of changes
pub fn diff_server_config(old: &ServerConfig, new: &ServerConfig, changes: &mut Vec<ConfigChange>) {
    // host and port are NOT hot-updatable (require restart)
    if old.host != new.host {
        changes.push(ConfigChange::new(
            "server.host",
            json!(old.host),
            json!(new.host),
            false, // requires restart
        ));
    }

    if old.port != new.port {
        changes.push(ConfigChange::new(
            "server.port",
            json!(old.port),
            json!(new.port),
            false, // requires restart
        ));
    }

    // Hot-updatable fields
    if old.max_tools_per_iteration != new.max_tools_per_iteration {
        changes.push(ConfigChange::new(
            "server.max_tools_per_iteration",
            json!(old.max_tools_per_iteration),
            json!(new.max_tools_per_iteration),
            true,
        ));
    }

    if old.tool_call_max_retries != new.tool_call_max_retries {
        changes.push(ConfigChange::new(
            "server.tool_call_max_retries",
            json!(old.tool_call_max_retries),
            json!(new.tool_call_max_retries),
            true,
        ));
    }

    if old.tool_call_retry_delay_ms != new.tool_call_retry_delay_ms {
        changes.push(ConfigChange::new(
            "server.tool_call_retry_delay_ms",
            json!(old.tool_call_retry_delay_ms),
            json!(new.tool_call_retry_delay_ms),
            true,
        ));
    }

    if old.max_plan_subtasks != new.max_plan_subtasks {
        changes.push(ConfigChange::new(
            "server.max_plan_subtasks",
            json!(old.max_plan_subtasks),
            json!(new.max_plan_subtasks),
            true,
        ));
    }

    if old.plan_timeout_secs != new.plan_timeout_secs {
        changes.push(ConfigChange::new(
            "server.plan_timeout_secs",
            json!(old.plan_timeout_secs),
            json!(new.plan_timeout_secs),
            true,
        ));
    }

    if old.subtask_max_retries != new.subtask_max_retries {
        changes.push(ConfigChange::new(
            "server.subtask_max_retries",
            json!(old.subtask_max_retries),
            json!(new.subtask_max_retries),
            true,
        ));
    }

    if old.subtask_react_max_iterations != new.subtask_react_max_iterations {
        changes.push(ConfigChange::new(
            "server.subtask_react_max_iterations",
            json!(old.subtask_react_max_iterations),
            json!(new.subtask_react_max_iterations),
            true,
        ));
    }

    if old.subtask_react_timeout_secs != new.subtask_react_timeout_secs {
        changes.push(ConfigChange::new(
            "server.subtask_react_timeout_secs",
            json!(old.subtask_react_timeout_secs),
            json!(new.subtask_react_timeout_secs),
            true,
        ));
    }
}

// ============================================================================
// Chat Config Diff
// ============================================================================

/// Compare two ChatConfig instances
fn diff_chat_config_inner(old: &ChatConfig, new: &ChatConfig, changes: &mut Vec<ConfigChange>) {
    if old.url != new.url {
        changes.push(ConfigChange::new(
            "chat.url",
            json!(old.url),
            json!(new.url),
            true, // hot-updatable but requires service reload
        ));
    }

    // Compare API keys (check if configured status changed or key changed)
    let old_has_key = old.get_api_key().is_some();
    let new_has_key = new.get_api_key().is_some();

    if old_has_key != new_has_key || (old_has_key && old.get_api_key() != new.get_api_key()) {
        changes.push(ConfigChange::new(
            "chat.api_key",
            if old_has_key {
                json!("***configured***")
            } else {
                serde_json::Value::Null
            },
            if new_has_key {
                json!("***configured***")
            } else {
                serde_json::Value::Null
            },
            true, // hot-updatable but requires service reload
        ));
    }
}

/// Compare chat config with optional handling
pub fn diff_chat_config(
    old: Option<&ChatConfig>,
    new: Option<&ChatConfig>,
    changes: &mut Vec<ConfigChange>,
) {
    diff_optional_config(old, new, "chat", changes, diff_chat_config_inner);
}

// ============================================================================
// Embedding Config Diff
// ============================================================================

/// Compare two EmbeddingConfig instances
fn diff_embedding_config_inner(
    old: &EmbeddingConfig,
    new: &EmbeddingConfig,
    changes: &mut Vec<ConfigChange>,
) {
    if old.url != new.url {
        changes.push(ConfigChange::new(
            "embedding.url",
            json!(old.url),
            json!(new.url),
            true, // hot-updatable but requires service reload
        ));
    }

    // Compare API keys
    let old_has_key = old.get_api_key().is_some();
    let new_has_key = new.get_api_key().is_some();

    if old_has_key != new_has_key || (old_has_key && old.get_api_key() != new.get_api_key()) {
        changes.push(ConfigChange::new(
            "embedding.api_key",
            if old_has_key {
                json!("***configured***")
            } else {
                serde_json::Value::Null
            },
            if new_has_key {
                json!("***configured***")
            } else {
                serde_json::Value::Null
            },
            true, // hot-updatable but requires service reload
        ));
    }
}

/// Compare embedding config with optional handling
pub fn diff_embedding_config(
    old: Option<&EmbeddingConfig>,
    new: Option<&EmbeddingConfig>,
    changes: &mut Vec<ConfigChange>,
) {
    diff_optional_config(old, new, "embedding", changes, diff_embedding_config_inner);
}

// ============================================================================
// Memory Config Diff
// ============================================================================

/// Compare two MemoryConfig instances
fn diff_memory_config_inner(
    old: &MemoryConfig,
    new: &MemoryConfig,
    changes: &mut Vec<ConfigChange>,
) {
    // enable and database_path are NOT hot-updatable
    if old.enable != new.enable {
        changes.push(ConfigChange::new(
            "memory.enable",
            json!(old.enable),
            json!(new.enable),
            false, // requires restart
        ));
    }

    if old.database_path != new.database_path {
        changes.push(ConfigChange::new(
            "memory.database_path",
            json!(old.database_path),
            json!(new.database_path),
            false, // requires restart
        ));
    }

    // Hot-updatable fields
    if old.auto_summarize != new.auto_summarize {
        changes.push(ConfigChange::new(
            "memory.auto_summarize",
            json!(old.auto_summarize),
            json!(new.auto_summarize),
            true,
        ));
    }

    if old.summarization_strategy != new.summarization_strategy {
        changes.push(ConfigChange::new(
            "memory.summarization_strategy",
            json!(old.summarization_strategy.to_string()),
            json!(new.summarization_strategy.to_string()),
            true,
        ));
    }

    if old.summarize_threshold != new.summarize_threshold {
        changes.push(ConfigChange::new(
            "memory.summarize_threshold",
            json!(old.summarize_threshold),
            json!(new.summarize_threshold),
            true,
        ));
    }

    if old.max_stored_messages != new.max_stored_messages {
        changes.push(ConfigChange::new(
            "memory.max_stored_messages",
            json!(old.max_stored_messages),
            json!(new.max_stored_messages),
            true,
        ));
    }
}

/// Compare memory config with optional handling
pub fn diff_memory_config(
    old: Option<&MemoryConfig>,
    new: Option<&MemoryConfig>,
    changes: &mut Vec<ConfigChange>,
) {
    diff_optional_config(old, new, "memory", changes, diff_memory_config_inner);
}

// ============================================================================
// RAG Config Diff
// ============================================================================

/// Compare two RagConfig instances
fn diff_rag_config_inner(old: &RagConfig, new: &RagConfig, changes: &mut Vec<ConfigChange>) {
    if old.enable != new.enable {
        changes.push(ConfigChange::new(
            "rag.enable",
            json!(old.enable),
            json!(new.enable),
            true, // hot-updatable
        ));
    }

    // policy and context_window are not in UPDATABLE_FIELDS, so they require restart
    if old.policy != new.policy {
        changes.push(ConfigChange::new(
            "rag.policy",
            json!(old.policy.to_string()),
            json!(new.policy.to_string()),
            false, // requires restart
        ));
    }

    if old.context_window != new.context_window {
        changes.push(ConfigChange::new(
            "rag.context_window",
            json!(old.context_window),
            json!(new.context_window),
            false, // requires restart
        ));
    }
}

/// Compare rag config with optional handling
pub fn diff_rag_config(
    old: Option<&RagConfig>,
    new: Option<&RagConfig>,
    changes: &mut Vec<ConfigChange>,
) {
    diff_optional_config(old, new, "rag", changes, diff_rag_config_inner);
}

// ============================================================================
// Optional Config Helper
// ============================================================================

/// Helper function to compare optional configuration sections
///
/// Handles three cases:
/// - Both present: call the inner diff function
/// - One present, one absent: mark as section added/removed (requires restart)
/// - Both absent: no change
pub fn diff_optional_config<T, F>(
    old: Option<&T>,
    new: Option<&T>,
    section_name: &str,
    changes: &mut Vec<ConfigChange>,
    diff_fn: F,
) where
    F: Fn(&T, &T, &mut Vec<ConfigChange>),
{
    match (old, new) {
        (Some(o), Some(n)) => diff_fn(o, n, changes),
        (None, Some(_)) => {
            // Section was added
            changes.push(ConfigChange::new(
                format!("{} (section)", section_name),
                serde_json::Value::Null,
                json!("configured"),
                false, // adding a new section requires restart
            ));
        }
        (Some(_), None) => {
            // Section was removed
            changes.push(ConfigChange::new(
                format!("{} (section)", section_name),
                json!("configured"),
                serde_json::Value::Null,
                false, // removing a section requires restart
            ));
        }
        (None, None) => {
            // No change
        }
    }
}

// ============================================================================
// Main Diff Function
// ============================================================================

/// Compare two configurations and return all changes
///
/// # Arguments
///
/// * `old` - The current configuration
/// * `new` - The new configuration to compare against
///
/// # Returns
///
/// A vector of `ConfigChange` representing all differences between the configs
pub fn diff_configs(old: &Config, new: &Config) -> Vec<ConfigChange> {
    let mut changes = Vec::new();

    // Compare server config
    diff_server_config(&old.server, &new.server, &mut changes);

    // Compare optional configs
    diff_chat_config(old.chat.as_ref(), new.chat.as_ref(), &mut changes);
    diff_embedding_config(old.embedding.as_ref(), new.embedding.as_ref(), &mut changes);
    diff_memory_config(old.memory.as_ref(), new.memory.as_ref(), &mut changes);
    diff_rag_config(old.rag.as_ref(), new.rag.as_ref(), &mut changes);

    changes
}

// ============================================================================
// Change Validation
// ============================================================================

/// Result of validating a configuration change
#[derive(Debug, Clone)]
pub struct ChangeValidationResult {
    /// Whether the change is valid
    pub is_valid: bool,
    /// Error message if invalid
    pub error: Option<String>,
}

impl ChangeValidationResult {
    /// Create a valid result
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            error: None,
        }
    }

    /// Create an invalid result with an error message
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            is_valid: false,
            error: Some(message.into()),
        }
    }
}

/// Validate a configuration change before applying it
///
/// # Arguments
///
/// * `change` - The configuration change to validate
///
/// # Returns
///
/// A `ChangeValidationResult` indicating whether the change is valid
pub fn validate_change(change: &ConfigChange) -> ChangeValidationResult {
    // Check if the field is updatable
    if !change.is_hot_updatable {
        return ChangeValidationResult::invalid(format!(
            "Field '{}' is not hot-updatable and requires a restart",
            change.field
        ));
    }

    // Validate based on field path
    match change.field.as_str() {
        // Server config validations
        "server.max_tools_per_iteration" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(1..=50).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "server.max_tools_per_iteration must be between 1 and 50",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.max_tools_per_iteration must be a positive integer",
                );
            }
        }
        "server.tool_call_max_retries" => {
            if let Some(val) = change.new_value.as_u64() {
                if val > 10 {
                    return ChangeValidationResult::invalid(
                        "server.tool_call_max_retries must be between 0 and 10",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.tool_call_max_retries must be a non-negative integer",
                );
            }
        }
        "server.tool_call_retry_delay_ms" => {
            if let Some(val) = change.new_value.as_u64() {
                if val > 10000 {
                    return ChangeValidationResult::invalid(
                        "server.tool_call_retry_delay_ms must be between 0 and 10000",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.tool_call_retry_delay_ms must be a non-negative integer",
                );
            }
        }
        "server.max_plan_subtasks" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(1..=50).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "server.max_plan_subtasks must be between 1 and 50",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.max_plan_subtasks must be a positive integer",
                );
            }
        }
        "server.plan_timeout_secs" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(60..=7200).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "server.plan_timeout_secs must be between 60 and 7200",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.plan_timeout_secs must be a positive integer",
                );
            }
        }
        "server.subtask_max_retries" => {
            if let Some(val) = change.new_value.as_u64() {
                if val > 10 {
                    return ChangeValidationResult::invalid(
                        "server.subtask_max_retries must be between 0 and 10",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.subtask_max_retries must be a non-negative integer",
                );
            }
        }
        "server.subtask_react_max_iterations" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(1..=20).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "server.subtask_react_max_iterations must be between 1 and 20",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.subtask_react_max_iterations must be a positive integer",
                );
            }
        }
        "server.subtask_react_timeout_secs" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(10..=600).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "server.subtask_react_timeout_secs must be between 10 and 600",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "server.subtask_react_timeout_secs must be a positive integer",
                );
            }
        }

        // Chat config validations
        "chat.url" => {
            if let Some(url) = change.new_value.as_str() {
                if url.is_empty() {
                    return ChangeValidationResult::invalid("chat.url cannot be empty");
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return ChangeValidationResult::invalid(
                        "chat.url must start with http:// or https://",
                    );
                }
            } else if !change.new_value.is_null() {
                return ChangeValidationResult::invalid("chat.url must be a string");
            }
        }
        "chat.api_key" => {
            // API key can be any string or null (to clear it)
            if !change.new_value.is_string() && !change.new_value.is_null() {
                return ChangeValidationResult::invalid("chat.api_key must be a string");
            }
        }

        // Embedding config validations
        "embedding.url" => {
            if let Some(url) = change.new_value.as_str() {
                if url.is_empty() {
                    return ChangeValidationResult::invalid("embedding.url cannot be empty");
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    return ChangeValidationResult::invalid(
                        "embedding.url must start with http:// or https://",
                    );
                }
            } else if !change.new_value.is_null() {
                return ChangeValidationResult::invalid("embedding.url must be a string");
            }
        }
        "embedding.api_key" => {
            // API key can be any string or null (to clear it)
            if !change.new_value.is_string() && !change.new_value.is_null() {
                return ChangeValidationResult::invalid("embedding.api_key must be a string");
            }
        }

        // Memory config validations
        "memory.auto_summarize" => {
            if !change.new_value.is_boolean() {
                return ChangeValidationResult::invalid("memory.auto_summarize must be a boolean");
            }
        }
        "memory.summarization_strategy" => {
            if let Some(strategy) = change.new_value.as_str() {
                if strategy != "Incremental" && strategy != "FullHistory" {
                    return ChangeValidationResult::invalid(
                        "memory.summarization_strategy must be 'Incremental' or 'FullHistory'",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "memory.summarization_strategy must be a string",
                );
            }
        }
        "memory.summarize_threshold" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(2..=100).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "memory.summarize_threshold must be between 2 and 100",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "memory.summarize_threshold must be a positive integer",
                );
            }
        }
        "memory.max_stored_messages" => {
            if let Some(val) = change.new_value.as_u64() {
                if !(5..=500).contains(&val) {
                    return ChangeValidationResult::invalid(
                        "memory.max_stored_messages must be between 5 and 500",
                    );
                }
            } else {
                return ChangeValidationResult::invalid(
                    "memory.max_stored_messages must be a positive integer",
                );
            }
        }

        // RAG config validations
        "rag.enable" => {
            if !change.new_value.is_boolean() {
                return ChangeValidationResult::invalid("rag.enable must be a boolean");
            }
        }

        // Unknown fields - allow if marked as hot-updatable
        _ => {}
    }

    ChangeValidationResult::valid()
}

// ============================================================================
// Change Application
// ============================================================================

/// Result of applying a single configuration change
#[derive(Debug, Clone)]
pub struct ApplyChangeResult {
    /// Whether the change was successfully applied
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Whether a service reload is required
    pub requires_reload: bool,
    /// Which service needs to be reloaded (if any)
    pub reload_service: Option<String>,
}

impl ApplyChangeResult {
    /// Create a successful result
    pub fn success() -> Self {
        Self {
            success: true,
            error: None,
            requires_reload: false,
            reload_service: None,
        }
    }

    /// Create a successful result that requires service reload
    pub fn success_with_reload(service: impl Into<String>) -> Self {
        Self {
            success: true,
            error: None,
            requires_reload: true,
            reload_service: Some(service.into()),
        }
    }

    /// Create a failed result
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(message.into()),
            requires_reload: false,
            reload_service: None,
        }
    }
}

/// Apply a single validated configuration change to the config
///
/// # Arguments
///
/// * `config` - The configuration to modify
/// * `change` - The validated configuration change to apply
///
/// # Returns
///
/// An `ApplyChangeResult` indicating whether the change was applied successfully
pub fn apply_single_change(config: &mut Config, change: &ConfigChange) -> ApplyChangeResult {
    // First validate the change
    let validation = validate_change(change);
    if !validation.is_valid {
        return ApplyChangeResult::failed(
            validation
                .error
                .unwrap_or_else(|| "Validation failed".to_string()),
        );
    }

    match change.field.as_str() {
        // Server config fields
        "server.max_tools_per_iteration" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.max_tools_per_iteration = val as usize;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.max_tools_per_iteration")
            }
        }
        "server.tool_call_max_retries" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.tool_call_max_retries = val as u32;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.tool_call_max_retries")
            }
        }
        "server.tool_call_retry_delay_ms" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.tool_call_retry_delay_ms = val;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.tool_call_retry_delay_ms")
            }
        }
        "server.max_plan_subtasks" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.max_plan_subtasks = val as usize;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.max_plan_subtasks")
            }
        }
        "server.plan_timeout_secs" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.plan_timeout_secs = val;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.plan_timeout_secs")
            }
        }
        "server.subtask_max_retries" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.subtask_max_retries = val as u32;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed("Invalid value type for server.subtask_max_retries")
            }
        }
        "server.subtask_react_max_iterations" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.subtask_react_max_iterations = val as u32;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed(
                    "Invalid value type for server.subtask_react_max_iterations",
                )
            }
        }
        "server.subtask_react_timeout_secs" => {
            if let Some(val) = change.new_value.as_u64() {
                config.server.subtask_react_timeout_secs = val;
                ApplyChangeResult::success()
            } else {
                ApplyChangeResult::failed(
                    "Invalid value type for server.subtask_react_timeout_secs",
                )
            }
        }

        // Chat config fields (require service reload)
        "chat.url" => {
            if let Some(chat) = config.chat.as_mut() {
                if let Some(url) = change.new_value.as_str() {
                    chat.url = url.to_string();
                    ApplyChangeResult::success_with_reload("chat")
                } else {
                    ApplyChangeResult::failed("Invalid value type for chat.url")
                }
            } else {
                ApplyChangeResult::failed("Chat configuration not initialized")
            }
        }
        "chat.api_key" => {
            // Note: ChatConfig has private api_key field, cannot be directly set
            // This requires a setter method to be added to ChatConfig
            ApplyChangeResult::failed("chat.api_key update requires ChatConfig setter method")
        }

        // Embedding config fields (require service reload)
        "embedding.url" => {
            if let Some(embedding) = config.embedding.as_mut() {
                if let Some(url) = change.new_value.as_str() {
                    embedding.url = url.to_string();
                    ApplyChangeResult::success_with_reload("embedding")
                } else {
                    ApplyChangeResult::failed("Invalid value type for embedding.url")
                }
            } else {
                ApplyChangeResult::failed("Embedding configuration not initialized")
            }
        }
        "embedding.api_key" => {
            // Note: EmbeddingConfig has private api_key field, cannot be directly set
            ApplyChangeResult::failed(
                "embedding.api_key update requires EmbeddingConfig setter method",
            )
        }

        // Memory config fields
        "memory.auto_summarize" => {
            if let Some(memory) = config.memory.as_mut() {
                if let Some(val) = change.new_value.as_bool() {
                    memory.auto_summarize = val;
                    ApplyChangeResult::success()
                } else {
                    ApplyChangeResult::failed("Invalid value type for memory.auto_summarize")
                }
            } else {
                ApplyChangeResult::failed("Memory configuration not initialized")
            }
        }
        "memory.summarization_strategy" => {
            if let Some(memory) = config.memory.as_mut() {
                if let Some(strategy_str) = change.new_value.as_str() {
                    match strategy_str {
                        "Incremental" => {
                            memory.summarization_strategy = SummarizationStrategy::Incremental;
                            ApplyChangeResult::success()
                        }
                        "FullHistory" => {
                            memory.summarization_strategy = SummarizationStrategy::FullHistory;
                            ApplyChangeResult::success()
                        }
                        _ => ApplyChangeResult::failed(
                            "Invalid value for memory.summarization_strategy",
                        ),
                    }
                } else {
                    ApplyChangeResult::failed(
                        "Invalid value type for memory.summarization_strategy",
                    )
                }
            } else {
                ApplyChangeResult::failed("Memory configuration not initialized")
            }
        }
        "memory.summarize_threshold" => {
            if let Some(memory) = config.memory.as_mut() {
                if let Some(val) = change.new_value.as_u64() {
                    memory.summarize_threshold = val as u32;
                    ApplyChangeResult::success()
                } else {
                    ApplyChangeResult::failed("Invalid value type for memory.summarize_threshold")
                }
            } else {
                ApplyChangeResult::failed("Memory configuration not initialized")
            }
        }
        "memory.max_stored_messages" => {
            if let Some(memory) = config.memory.as_mut() {
                if let Some(val) = change.new_value.as_u64() {
                    memory.max_stored_messages = val as u32;
                    ApplyChangeResult::success()
                } else {
                    ApplyChangeResult::failed("Invalid value type for memory.max_stored_messages")
                }
            } else {
                ApplyChangeResult::failed("Memory configuration not initialized")
            }
        }

        // RAG config fields
        "rag.enable" => {
            if let Some(rag) = config.rag.as_mut() {
                if let Some(val) = change.new_value.as_bool() {
                    rag.enable = val;
                    ApplyChangeResult::success()
                } else {
                    ApplyChangeResult::failed("Invalid value type for rag.enable")
                }
            } else {
                ApplyChangeResult::failed("RAG configuration not initialized")
            }
        }

        // Section changes (not hot-updatable)
        field if field.ends_with(" (section)") => ApplyChangeResult::failed(format!(
            "Configuration section '{}' cannot be added or removed at runtime",
            field.trim_end_matches(" (section)")
        )),

        // Unknown fields
        _ => ApplyChangeResult::failed(format!("Unknown field: {}", change.field)),
    }
}

/// Apply multiple configuration changes
///
/// # Arguments
///
/// * `config` - The configuration to modify
/// * `changes` - The list of changes to apply
///
/// # Returns
///
/// A vector of tuples containing the field path and its apply result
pub fn apply_changes(
    config: &mut Config,
    changes: &[ConfigChange],
) -> Vec<(String, ApplyChangeResult)> {
    changes
        .iter()
        .map(|change| {
            let result = apply_single_change(config, change);
            (change.field.clone(), result)
        })
        .collect()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_updatable_field() {
        assert!(is_updatable_field("server.max_tools_per_iteration"));
        assert!(is_updatable_field("chat.url"));
        assert!(is_updatable_field("chat.api_key"));
        assert!(is_updatable_field("memory.auto_summarize"));
        assert!(is_updatable_field("rag.enable"));

        assert!(!is_updatable_field("server.host"));
        assert!(!is_updatable_field("server.port"));
        assert!(!is_updatable_field("memory.enable"));
        assert!(!is_updatable_field("unknown.field"));
    }

    #[test]
    fn test_config_change_new() {
        let change = ConfigChange::new("server.max_tools_per_iteration", json!(5), json!(10), true);

        assert_eq!(change.field, "server.max_tools_per_iteration");
        assert_eq!(change.old_value, json!(5));
        assert_eq!(change.new_value, json!(10));
        assert!(change.is_hot_updatable);
    }

    #[test]
    fn test_diff_server_config_no_changes() {
        let config = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3389,
            max_tools_per_iteration: 5,
            tool_call_max_retries: 2,
            tool_call_retry_delay_ms: 500,
            max_plan_subtasks: 10,
            plan_timeout_secs: 600,
            subtask_max_retries: 2,
            subtask_react_max_iterations: 5,
            subtask_react_timeout_secs: 60,
        };

        let mut changes = Vec::new();
        diff_server_config(&config, &config, &mut changes);

        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_server_config_hot_updatable() {
        let old = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3389,
            max_tools_per_iteration: 5,
            tool_call_max_retries: 2,
            tool_call_retry_delay_ms: 500,
            max_plan_subtasks: 10,
            plan_timeout_secs: 600,
            subtask_max_retries: 2,
            subtask_react_max_iterations: 5,
            subtask_react_timeout_secs: 60,
        };

        let new = ServerConfig {
            max_tools_per_iteration: 10,
            tool_call_max_retries: 3,
            ..old.clone()
        };

        let mut changes = Vec::new();
        diff_server_config(&old, &new, &mut changes);

        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| c.is_hot_updatable));
        assert!(
            changes
                .iter()
                .any(|c| c.field == "server.max_tools_per_iteration")
        );
        assert!(
            changes
                .iter()
                .any(|c| c.field == "server.tool_call_max_retries")
        );
    }

    #[test]
    fn test_diff_server_config_requires_restart() {
        let old = ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3389,
            max_tools_per_iteration: 5,
            tool_call_max_retries: 2,
            tool_call_retry_delay_ms: 500,
            max_plan_subtasks: 10,
            plan_timeout_secs: 600,
            subtask_max_retries: 2,
            subtask_react_max_iterations: 5,
            subtask_react_timeout_secs: 60,
        };

        let new = ServerConfig {
            host: "0.0.0.0".to_string(),
            port: 8080,
            ..old.clone()
        };

        let mut changes = Vec::new();
        diff_server_config(&old, &new, &mut changes);

        assert_eq!(changes.len(), 2);
        assert!(changes.iter().all(|c| !c.is_hot_updatable));
        assert!(changes.iter().any(|c| c.field == "server.host"));
        assert!(changes.iter().any(|c| c.field == "server.port"));
    }

    #[test]
    fn test_diff_optional_config_both_none() {
        let mut changes = Vec::new();
        diff_chat_config(None, None, &mut changes);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_optional_config_section_added() {
        // Use MemoryConfig instead of ChatConfig since ChatConfig has private fields
        let mut changes = Vec::new();
        let new_config = MemoryConfig::default();
        diff_optional_config(
            None,
            Some(&new_config),
            "memory",
            &mut changes,
            diff_memory_config_inner,
        );

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "memory (section)");
        assert!(!changes[0].is_hot_updatable);
    }

    #[test]
    fn test_diff_optional_config_section_removed() {
        // Use MemoryConfig instead of ChatConfig since ChatConfig has private fields
        let mut changes = Vec::new();
        let old_config = MemoryConfig::default();
        diff_optional_config(
            Some(&old_config),
            None,
            "memory",
            &mut changes,
            diff_memory_config_inner,
        );

        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].field, "memory (section)");
        assert!(!changes[0].is_hot_updatable);
    }

    #[test]
    fn test_diff_configs_empty() {
        let config = Config::default();
        let changes = diff_configs(&config, &config);
        assert!(changes.is_empty());
    }

    #[test]
    fn test_diff_configs_multiple_changes() {
        let old = Config::default();
        let mut new = Config::default();

        // Change server config
        new.server.max_tools_per_iteration = 10;

        // Add memory config to new
        new.memory = Some(MemoryConfig::default());

        let changes = diff_configs(&old, &new);

        // Should have at least 2 changes: server.max_tools_per_iteration and memory section
        assert!(changes.len() >= 2);
        assert!(
            changes
                .iter()
                .any(|c| c.field == "server.max_tools_per_iteration")
        );
        assert!(changes.iter().any(|c| c.field == "memory (section)"));
    }

    // ========================================================================
    // Validation Tests (T3.2)
    // ========================================================================

    #[test]
    fn test_validate_change_non_hot_updatable() {
        let change = ConfigChange::new("server.host", json!("127.0.0.1"), json!("0.0.0.0"), false);
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("not hot-updatable"));
    }

    #[test]
    fn test_validate_change_server_max_tools_valid() {
        let change = ConfigChange::new("server.max_tools_per_iteration", json!(5), json!(10), true);
        let result = validate_change(&change);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_change_server_max_tools_invalid_range() {
        let change = ConfigChange::new(
            "server.max_tools_per_iteration",
            json!(5),
            json!(100), // Too high
            true,
        );
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("between 1 and 50"));
    }

    #[test]
    fn test_validate_change_server_max_tools_invalid_type() {
        let change = ConfigChange::new(
            "server.max_tools_per_iteration",
            json!(5),
            json!("not a number"),
            true,
        );
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("positive integer"));
    }

    #[test]
    fn test_validate_change_chat_url_valid() {
        let change = ConfigChange::new(
            "chat.url",
            json!("http://old.example.com/v1"),
            json!("https://new.example.com/v1"),
            true,
        );
        let result = validate_change(&change);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_change_chat_url_invalid_empty() {
        let change = ConfigChange::new("chat.url", json!("http://example.com/v1"), json!(""), true);
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("cannot be empty"));
    }

    #[test]
    fn test_validate_change_chat_url_invalid_protocol() {
        let change = ConfigChange::new(
            "chat.url",
            json!("http://example.com/v1"),
            json!("ftp://invalid.com"),
            true,
        );
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("http://"));
    }

    #[test]
    fn test_validate_change_memory_auto_summarize_valid() {
        let change = ConfigChange::new("memory.auto_summarize", json!(false), json!(true), true);
        let result = validate_change(&change);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_change_memory_auto_summarize_invalid_type() {
        let change = ConfigChange::new("memory.auto_summarize", json!(false), json!("true"), true);
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("boolean"));
    }

    #[test]
    fn test_validate_change_memory_strategy_valid() {
        let change = ConfigChange::new(
            "memory.summarization_strategy",
            json!("Incremental"),
            json!("FullHistory"),
            true,
        );
        let result = validate_change(&change);
        assert!(result.is_valid);
    }

    #[test]
    fn test_validate_change_memory_strategy_invalid() {
        let change = ConfigChange::new(
            "memory.summarization_strategy",
            json!("Incremental"),
            json!("InvalidStrategy"),
            true,
        );
        let result = validate_change(&change);
        assert!(!result.is_valid);
        assert!(result.error.unwrap().contains("Incremental"));
    }

    #[test]
    fn test_validate_change_rag_enable_valid() {
        let change = ConfigChange::new("rag.enable", json!(false), json!(true), true);
        let result = validate_change(&change);
        assert!(result.is_valid);
    }

    // ========================================================================
    // Apply Change Tests (T3.1)
    // ========================================================================

    #[test]
    fn test_apply_single_change_server_max_tools() {
        let mut config = Config::default();
        let change = ConfigChange::new("server.max_tools_per_iteration", json!(5), json!(10), true);

        let result = apply_single_change(&mut config, &change);
        assert!(result.success);
        assert!(!result.requires_reload);
        assert_eq!(config.server.max_tools_per_iteration, 10);
    }

    #[test]
    fn test_apply_single_change_server_tool_call_max_retries() {
        let mut config = Config::default();
        let change = ConfigChange::new("server.tool_call_max_retries", json!(2), json!(5), true);

        let result = apply_single_change(&mut config, &change);
        assert!(result.success);
        assert_eq!(config.server.tool_call_max_retries, 5);
    }

    #[test]
    fn test_apply_single_change_server_plan_timeout() {
        let mut config = Config::default();
        let change = ConfigChange::new("server.plan_timeout_secs", json!(600), json!(1200), true);

        let result = apply_single_change(&mut config, &change);
        assert!(result.success);
        assert_eq!(config.server.plan_timeout_secs, 1200);
    }

    #[test]
    fn test_apply_single_change_validation_failure() {
        let mut config = Config::default();
        let change = ConfigChange::new(
            "server.max_tools_per_iteration",
            json!(5),
            json!(100), // Invalid: too high
            true,
        );

        let result = apply_single_change(&mut config, &change);
        assert!(!result.success);
        assert!(result.error.is_some());
        // Config should not be modified
        assert_ne!(config.server.max_tools_per_iteration, 100);
    }

    #[test]
    fn test_apply_single_change_memory_auto_summarize() {
        let mut config = Config {
            memory: Some(MemoryConfig::default()),
            ..Config::default()
        };
        let change = ConfigChange::new("memory.auto_summarize", json!(true), json!(false), true);

        let result = apply_single_change(&mut config, &change);
        assert!(result.success);
        assert!(!config.memory.as_ref().unwrap().auto_summarize);
    }

    #[test]
    fn test_apply_single_change_memory_summarization_strategy() {
        let mut config = Config {
            memory: Some(MemoryConfig::default()),
            ..Config::default()
        };
        let change = ConfigChange::new(
            "memory.summarization_strategy",
            json!("Incremental"),
            json!("FullHistory"),
            true,
        );

        let result = apply_single_change(&mut config, &change);
        assert!(result.success);
        assert_eq!(
            config.memory.as_ref().unwrap().summarization_strategy,
            SummarizationStrategy::FullHistory
        );
    }

    #[test]
    fn test_apply_single_change_memory_not_initialized() {
        let mut config = Config::default(); // memory is None
        let change = ConfigChange::new("memory.auto_summarize", json!(false), json!(true), true);

        let result = apply_single_change(&mut config, &change);
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not initialized"));
    }

    #[test]
    fn test_apply_single_change_unknown_field() {
        let mut config = Config::default();
        let change = ConfigChange::new("unknown.field", json!(1), json!(2), true);

        let result = apply_single_change(&mut config, &change);
        assert!(!result.success);
        assert!(result.error.unwrap().contains("Unknown field"));
    }

    #[test]
    fn test_apply_single_change_non_hot_updatable() {
        let mut config = Config::default();
        let change = ConfigChange::new(
            "server.host",
            json!("127.0.0.1"),
            json!("0.0.0.0"),
            false, // Not hot-updatable
        );

        let result = apply_single_change(&mut config, &change);
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not hot-updatable"));
    }

    // ========================================================================
    // Apply Multiple Changes Tests
    // ========================================================================

    #[test]
    fn test_apply_changes_multiple() {
        let mut config = Config {
            memory: Some(MemoryConfig::default()),
            ..Config::default()
        };

        let changes = vec![
            ConfigChange::new("server.max_tools_per_iteration", json!(5), json!(10), true),
            ConfigChange::new("server.tool_call_max_retries", json!(2), json!(5), true),
            ConfigChange::new("memory.auto_summarize", json!(true), json!(false), true),
        ];

        let results = apply_changes(&mut config, &changes);

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|(_, r)| r.success));
        assert_eq!(config.server.max_tools_per_iteration, 10);
        assert_eq!(config.server.tool_call_max_retries, 5);
        assert!(!config.memory.as_ref().unwrap().auto_summarize);
    }

    #[test]
    fn test_apply_changes_partial_success() {
        let mut config = Config::default(); // No memory config

        let changes = vec![
            ConfigChange::new("server.max_tools_per_iteration", json!(5), json!(10), true),
            ConfigChange::new("memory.auto_summarize", json!(true), json!(false), true), // Will fail
        ];

        let results = apply_changes(&mut config, &changes);

        assert_eq!(results.len(), 2);
        assert!(results[0].1.success); // Server change succeeded
        assert!(!results[1].1.success); // Memory change failed
        assert_eq!(config.server.max_tools_per_iteration, 10);
    }
}
