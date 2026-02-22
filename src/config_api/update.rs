//! Configuration update logic
//!
//! This module provides functions for applying configuration updates.

use std::collections::HashMap;

use super::types::{
    ChatConfigUpdate, ConfigUpdateRequest, ConfigUpdateResponse, EmbeddingConfigUpdate,
    LantaiAutoMemoryConfigUpdate, MemoryConfigUpdate, RagConfigUpdate, SIDE_EFFECT_FIELDS,
    ServerConfigUpdate, SubagentConfigUpdate,
};
use crate::{
    config::{AriesLantaiConfig, ChatConfig, Config, EmbeddingConfig, MemoryConfig, RagConfig},
    subagent::SubAgentSystemConfig,
};

/// Result of applying a configuration update
pub struct UpdateResult {
    /// Fields that were successfully updated
    pub updated_fields: Vec<String>,
    /// Fields that require side effects (service reload)
    pub side_effect_fields: Vec<String>,
    /// Fields that failed to update
    pub failed_fields: HashMap<String, String>,
}

impl UpdateResult {
    pub fn new() -> Self {
        Self {
            updated_fields: Vec::new(),
            side_effect_fields: Vec::new(),
            failed_fields: HashMap::new(),
        }
    }

    pub fn add_updated(&mut self, field: &str) {
        self.updated_fields.push(field.to_string());
        // Check if this field requires side effects
        if SIDE_EFFECT_FIELDS.contains(&field) {
            self.side_effect_fields.push(field.to_string());
        }
    }

    pub fn add_failed(&mut self, field: &str, reason: &str) {
        self.failed_fields
            .insert(field.to_string(), reason.to_string());
    }

    pub fn into_response(self) -> ConfigUpdateResponse {
        let has_failures = !self.failed_fields.is_empty();
        let mut response = if has_failures {
            ConfigUpdateResponse::partial(self.updated_fields, self.failed_fields)
        } else {
            ConfigUpdateResponse::success(self.updated_fields)
        };

        // Add side effect actions
        for field in self.side_effect_fields {
            let action = if field.starts_with("chat.") {
                "chat_service_will_be_reloaded"
            } else if field.starts_with("embedding.") {
                "embedding_service_will_be_reloaded"
            } else {
                "service_reload_required"
            };
            response = response.with_action(&field, action);
        }

        response
    }
}

impl Default for UpdateResult {
    fn default() -> Self {
        Self::new()
    }
}

/// Apply configuration updates to the config object
///
/// This function modifies the config in place and returns information about
/// which fields were updated successfully.
pub fn apply_config_update(
    config: &mut Config,
    request: &ConfigUpdateRequest,
    validated_fields: &[String],
) -> UpdateResult {
    let mut result = UpdateResult::new();

    // Apply server config updates
    if let Some(ref server_update) = request.server {
        apply_server_config_update(
            &mut config.server,
            server_update,
            validated_fields,
            &mut result,
        );
    }

    // Apply chat config updates
    if let Some(ref chat_update) = request.chat {
        apply_chat_config_update(&mut config.chat, chat_update, validated_fields, &mut result);
    }

    // Apply embedding config updates
    if let Some(ref embedding_update) = request.embedding {
        apply_embedding_config_update(
            &mut config.embedding,
            embedding_update,
            validated_fields,
            &mut result,
        );
    }

    // Apply memory config updates
    if let Some(ref memory_update) = request.memory {
        apply_memory_config_update(
            &mut config.memory,
            memory_update,
            validated_fields,
            &mut result,
        );
    }

    // Apply rag config updates
    if let Some(ref rag_update) = request.rag {
        apply_rag_config_update(&mut config.rag, rag_update, validated_fields, &mut result);
    }

    // Apply subagent config updates
    if let Some(ref subagent_update) = request.subagent {
        apply_subagent_config_update(
            &mut config.subagent,
            subagent_update,
            validated_fields,
            &mut result,
        );
    }

    // Apply lantai auto memory config updates
    if let Some(ref lantai_update) = request.lantai_auto_memory {
        apply_lantai_auto_memory_config_update(
            &mut config.lantai,
            lantai_update,
            validated_fields,
            &mut result,
        );
    }

    result
}

/// Apply server configuration updates
fn apply_server_config_update(
    server: &mut crate::config::ServerConfig,
    update: &ServerConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    if let Some(val) = update.max_tools_per_iteration {
        let field = "server.max_tools_per_iteration";
        if validated_fields.contains(&field.to_string()) {
            server.max_tools_per_iteration = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.tool_call_max_retries {
        let field = "server.tool_call_max_retries";
        if validated_fields.contains(&field.to_string()) {
            server.tool_call_max_retries = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.tool_call_retry_delay_ms {
        let field = "server.tool_call_retry_delay_ms";
        if validated_fields.contains(&field.to_string()) {
            server.tool_call_retry_delay_ms = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.max_plan_subtasks {
        let field = "server.max_plan_subtasks";
        if validated_fields.contains(&field.to_string()) {
            server.max_plan_subtasks = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.plan_timeout_secs {
        let field = "server.plan_timeout_secs";
        if validated_fields.contains(&field.to_string()) {
            server.plan_timeout_secs = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.subtask_max_retries {
        let field = "server.subtask_max_retries";
        if validated_fields.contains(&field.to_string()) {
            server.subtask_max_retries = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.subtask_react_max_iterations {
        let field = "server.subtask_react_max_iterations";
        if validated_fields.contains(&field.to_string()) {
            server.subtask_react_max_iterations = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.subtask_react_timeout_secs {
        let field = "server.subtask_react_timeout_secs";
        if validated_fields.contains(&field.to_string()) {
            server.subtask_react_timeout_secs = val;
            result.add_updated(field);
        }
    }
}

/// Apply chat configuration updates
fn apply_chat_config_update(
    chat: &mut Option<ChatConfig>,
    update: &ChatConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    // Chat config must exist to be updated
    let Some(chat_config) = chat.as_mut() else {
        if update.url.is_some() {
            result.add_failed("chat.url", "Chat configuration not initialized");
        }
        if update.api_key.is_some() {
            result.add_failed("chat.api_key", "Chat configuration not initialized");
        }
        if update.model.is_some() {
            result.add_failed("chat.model", "Chat configuration not initialized");
        }
        if update.model_context_size.is_some() {
            result.add_failed(
                "chat.model_context_size",
                "Chat configuration not initialized",
            );
        }
        return;
    };

    if let Some(ref url) = update.url {
        let field = "chat.url";
        if validated_fields.contains(&field.to_string()) {
            chat_config.url = url.clone();
            result.add_updated(field);
        }
    }

    if let Some(ref api_key) = update.api_key {
        let field = "chat.api_key";
        if validated_fields.contains(&field.to_string()) {
            chat_config.set_api_key(api_key.clone());
            result.add_updated(field);
        }
    }

    if let Some(ref model) = update.model {
        let field = "chat.model";
        if validated_fields.contains(&field.to_string()) {
            chat_config.model = model.clone();
            result.add_updated(field);
        }
    }

    if let Some(val) = update.model_context_size {
        let field = "chat.model_context_size";
        if validated_fields.contains(&field.to_string()) {
            chat_config.model_context_size = val;
            result.add_updated(field);
        }
    }
}

/// Apply embedding configuration updates
///
/// If the embedding config does not exist and a URL is provided, creates a new config.
fn apply_embedding_config_update(
    embedding: &mut Option<EmbeddingConfig>,
    update: &EmbeddingConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    // Auto-create embedding config if it doesn't exist and URL is provided
    if embedding.is_none() {
        if let Some(ref url) = update.url {
            if validated_fields.contains(&"embedding.url".to_string()) {
                *embedding = Some(EmbeddingConfig::new_with_url(url.clone()));
                result.add_updated("embedding.url");
            }
        } else {
            // Can't create without URL
            if update.api_key.is_some() {
                result.add_failed(
                    "embedding.api_key",
                    "Embedding configuration not initialized, provide URL first",
                );
            }
            return;
        }
    }

    let embedding_config = embedding.as_mut().unwrap();

    // Update URL (skip if already set above during creation)
    if let Some(ref url) = update.url {
        let field = "embedding.url";
        if validated_fields.contains(&field.to_string())
            && !result.updated_fields.contains(&field.to_string())
        {
            embedding_config.url = url.clone();
            result.add_updated(field);
        }
    }

    if let Some(ref api_key) = update.api_key {
        let field = "embedding.api_key";
        if validated_fields.contains(&field.to_string()) {
            embedding_config.set_api_key(api_key.clone());
            result.add_updated(field);
        }
    }
}

/// Apply memory configuration updates
fn apply_memory_config_update(
    memory: &mut Option<MemoryConfig>,
    update: &MemoryConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    // Memory config must exist to be updated
    let Some(memory_config) = memory.as_mut() else {
        if update.auto_summarize.is_some() {
            result.add_failed(
                "memory.auto_summarize",
                "Memory configuration not initialized",
            );
        }
        if update.summarization_strategy.is_some() {
            result.add_failed(
                "memory.summarization_strategy",
                "Memory configuration not initialized",
            );
        }
        if update.summarize_threshold.is_some() {
            result.add_failed(
                "memory.summarize_threshold",
                "Memory configuration not initialized",
            );
        }
        if update.max_stored_messages.is_some() {
            result.add_failed(
                "memory.max_stored_messages",
                "Memory configuration not initialized",
            );
        }
        return;
    };

    if let Some(val) = update.auto_summarize {
        let field = "memory.auto_summarize";
        if validated_fields.contains(&field.to_string()) {
            memory_config.auto_summarize = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.summarization_strategy {
        let field = "memory.summarization_strategy";
        if validated_fields.contains(&field.to_string()) {
            memory_config.summarization_strategy = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.summarize_threshold {
        let field = "memory.summarize_threshold";
        if validated_fields.contains(&field.to_string()) {
            memory_config.summarize_threshold = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.max_stored_messages {
        let field = "memory.max_stored_messages";
        if validated_fields.contains(&field.to_string()) {
            memory_config.max_stored_messages = val;
            result.add_updated(field);
        }
    }
}

/// Apply RAG configuration updates
fn apply_rag_config_update(
    rag: &mut Option<RagConfig>,
    update: &RagConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    // RAG config must exist to be updated
    let Some(rag_config) = rag.as_mut() else {
        if update.enable.is_some() {
            result.add_failed("rag.enable", "RAG configuration not initialized");
        }
        return;
    };

    if let Some(val) = update.enable {
        let field = "rag.enable";
        if validated_fields.contains(&field.to_string()) {
            rag_config.enable = val;
            result.add_updated(field);
        }
    }
}

/// Apply Sub-Agent configuration updates
fn apply_subagent_config_update(
    subagent: &mut Option<SubAgentSystemConfig>,
    update: &SubagentConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    // Sub-Agent config must exist to be updated
    let Some(subagent_config) = subagent.as_mut() else {
        if update.execution_mode.is_some() {
            result.add_failed(
                "subagent.execution_mode",
                "Sub-Agent configuration not initialized",
            );
        }
        if update.parallel_mode.is_some() {
            result.add_failed(
                "subagent.parallel_mode",
                "Sub-Agent configuration not initialized",
            );
        }
        return;
    };

    // Update execution_mode
    if let Some(ref mode) = update.execution_mode {
        let field = "subagent.execution_mode";
        if validated_fields.contains(&field.to_string()) {
            // Validate execution_mode value
            if mode == "direct" || mode == "subagent" {
                subagent_config.execution_mode = mode.clone();
                result.add_updated(field);
            } else {
                result.add_failed(
                    field,
                    "Invalid execution_mode. Must be 'direct' or 'subagent'",
                );
            }
        }
    }

    // Update parallel_mode (in subtask_executor config)
    if let Some(ref mode) = update.parallel_mode {
        let field = "subagent.parallel_mode";
        if validated_fields.contains(&field.to_string()) {
            // Validate parallel_mode value
            let valid_modes = ["auto", "sequential", "manual"];
            if valid_modes.contains(&mode.as_str()) {
                subagent_config.subtask_executor.parallel_mode = mode.clone();
                result.add_updated(field);
            } else {
                result.add_failed(
                    field,
                    "Invalid parallel_mode. Must be 'auto', 'sequential', or 'manual'",
                );
            }
        }
    }
}

/// Apply lantai auto memory configuration updates
fn apply_lantai_auto_memory_config_update(
    lantai: &mut Option<AriesLantaiConfig>,
    update: &LantaiAutoMemoryConfigUpdate,
    validated_fields: &[String],
    result: &mut UpdateResult,
) {
    let Some(lantai_config) = lantai.as_mut() else {
        if update.auto_summary.is_some()
            || update.checkpoint_token_ratio.is_some()
            || update.embedding_model.is_some()
            || update.embedding_dimensions.is_some()
            || update.embedding_batch_size.is_some()
        {
            result.add_failed("lantai_auto_memory", "Lantai configuration not initialized");
        }
        return;
    };

    if let Some(val) = update.auto_summary {
        let field = "lantai_auto_memory.auto_summary";
        if validated_fields.contains(&field.to_string()) {
            lantai_config.auto_memory.auto_summary = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.checkpoint_token_ratio {
        let field = "lantai_auto_memory.checkpoint_token_ratio";
        if validated_fields.contains(&field.to_string()) {
            lantai_config.auto_memory.checkpoint_token_ratio = val;
            result.add_updated(field);
        }
    }

    if let Some(ref model) = update.embedding_model {
        let field = "lantai_auto_memory.embedding_model";
        if validated_fields.contains(&field.to_string()) {
            lantai_config.embedding.model = model.clone();
            result.add_updated(field);
        }
    }

    if let Some(val) = update.embedding_dimensions {
        let field = "lantai_auto_memory.embedding_dimensions";
        if validated_fields.contains(&field.to_string()) {
            lantai_config.embedding.dimensions = val;
            result.add_updated(field);
        }
    }

    if let Some(val) = update.embedding_batch_size {
        let field = "lantai_auto_memory.embedding_batch_size";
        if validated_fields.contains(&field.to_string()) {
            lantai_config.embedding.batch_size = val;
            result.add_updated(field);
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_result_new() {
        let result = UpdateResult::new();
        assert!(result.updated_fields.is_empty());
        assert!(result.side_effect_fields.is_empty());
        assert!(result.failed_fields.is_empty());
    }

    #[test]
    fn test_update_result_add_updated() {
        let mut result = UpdateResult::new();
        result.add_updated("server.max_tools_per_iteration");
        assert_eq!(result.updated_fields.len(), 1);
        assert!(result.side_effect_fields.is_empty());
    }

    #[test]
    fn test_update_result_add_side_effect_field() {
        let mut result = UpdateResult::new();
        // embedding.url is in SIDE_EFFECT_FIELDS and triggers service reload
        result.add_updated("embedding.url");
        assert_eq!(result.updated_fields.len(), 1);
        assert_eq!(result.side_effect_fields.len(), 1);
        assert!(
            result
                .side_effect_fields
                .contains(&"embedding.url".to_string())
        );
    }

    #[test]
    fn test_update_result_into_response_success() {
        let mut result = UpdateResult::new();
        result.add_updated("server.max_tools_per_iteration");
        result.add_updated("server.plan_timeout_secs");

        let response = result.into_response();
        assert!(response.success);
        assert_eq!(response.updated_fields.len(), 2);
        assert!(response.failed_fields.is_empty());
    }

    #[test]
    fn test_update_result_into_response_with_side_effects() {
        let mut result = UpdateResult::new();
        // embedding.url is in SIDE_EFFECT_FIELDS and triggers service reload
        result.add_updated("embedding.url");

        let response = result.into_response();
        assert!(response.success);
        assert!(!response.requires_action.is_empty());
        assert!(response.requires_action.contains_key("embedding.url"));
    }

    #[test]
    fn test_apply_server_config_update() {
        let mut config = Config::default();
        let update = ServerConfigUpdate {
            max_tools_per_iteration: Some(10),
            tool_call_max_retries: Some(5),
            tool_call_retry_delay_ms: None,
            max_plan_subtasks: None,
            plan_timeout_secs: Some(900),
            subtask_max_retries: None,
            subtask_react_max_iterations: None,
            subtask_react_timeout_secs: None,
        };

        let validated_fields = vec![
            "server.max_tools_per_iteration".to_string(),
            "server.tool_call_max_retries".to_string(),
            "server.plan_timeout_secs".to_string(),
        ];

        let mut result = UpdateResult::new();
        apply_server_config_update(&mut config.server, &update, &validated_fields, &mut result);

        assert_eq!(result.updated_fields.len(), 3);
        assert_eq!(config.server.max_tools_per_iteration, 10);
        assert_eq!(config.server.tool_call_max_retries, 5);
        assert_eq!(config.server.plan_timeout_secs, 900);
    }

    #[test]
    fn test_apply_memory_config_update() {
        let mut config = Config {
            memory: Some(MemoryConfig::default()),
            ..Config::default()
        };

        let update = MemoryConfigUpdate {
            auto_summarize: Some(false),
            summarization_strategy: None,
            summarize_threshold: Some(20),
            max_stored_messages: Some(50),
        };

        let validated_fields = vec![
            "memory.auto_summarize".to_string(),
            "memory.summarize_threshold".to_string(),
            "memory.max_stored_messages".to_string(),
        ];

        let mut result = UpdateResult::new();
        apply_memory_config_update(&mut config.memory, &update, &validated_fields, &mut result);

        assert_eq!(result.updated_fields.len(), 3);
        let memory = config.memory.unwrap();
        assert!(!memory.auto_summarize);
        assert_eq!(memory.summarize_threshold, 20);
        assert_eq!(memory.max_stored_messages, 50);
    }

    #[test]
    fn test_apply_memory_config_update_not_initialized() {
        let mut config = Config::default(); // memory is None

        let update = MemoryConfigUpdate {
            auto_summarize: Some(true),
            summarization_strategy: None,
            summarize_threshold: None,
            max_stored_messages: None,
        };

        let validated_fields = vec!["memory.auto_summarize".to_string()];

        let mut result = UpdateResult::new();
        apply_memory_config_update(&mut config.memory, &update, &validated_fields, &mut result);

        assert!(result.updated_fields.is_empty());
        assert!(!result.failed_fields.is_empty());
        assert!(result.failed_fields.contains_key("memory.auto_summarize"));
    }
}
