//! Configuration validation logic
//!
//! This module provides validation functions for configuration update requests.

use super::types::{
    ChatConfigUpdate, ConfigUpdateRequest, EmbeddingConfigUpdate, MemoryConfigUpdate,
    RagConfigUpdate, ServerConfigUpdate, UPDATABLE_FIELDS,
};

/// Validation error for configuration updates
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

/// Result of validating a configuration update request
#[derive(Debug, Default)]
pub struct ValidationResult {
    /// Fields that passed validation
    pub valid_fields: Vec<String>,
    /// Fields that failed validation with error messages
    pub errors: Vec<ValidationError>,
}

impl ValidationResult {
    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    pub fn add_valid(&mut self, field: &str) {
        self.valid_fields.push(field.to_string());
    }

    pub fn add_error(&mut self, field: &str, message: &str) {
        self.errors.push(ValidationError {
            field: field.to_string(),
            message: message.to_string(),
        });
    }
}

/// Check if a field path is updatable
#[allow(dead_code)]
pub fn is_field_updatable(field_path: &str) -> bool {
    UPDATABLE_FIELDS.contains(&field_path)
}

/// Validate a configuration update request
pub fn validate_config_update(request: &ConfigUpdateRequest) -> ValidationResult {
    let mut result = ValidationResult::default();

    // Validate server config updates
    if let Some(ref server) = request.server {
        validate_server_config(server, &mut result);
    }

    // Validate chat config updates
    if let Some(ref chat) = request.chat {
        validate_chat_config(chat, &mut result);
    }

    // Validate embedding config updates
    if let Some(ref embedding) = request.embedding {
        validate_embedding_config(embedding, &mut result);
    }

    // Validate memory config updates
    if let Some(ref memory) = request.memory {
        validate_memory_config(memory, &mut result);
    }

    // Validate rag config updates
    if let Some(ref rag) = request.rag {
        validate_rag_config(rag, &mut result);
    }

    result
}

/// Validate server configuration updates
fn validate_server_config(server: &ServerConfigUpdate, result: &mut ValidationResult) {
    if let Some(val) = server.max_tools_per_iteration {
        let field = "server.max_tools_per_iteration";
        if !(1..=50).contains(&val) {
            result.add_error(field, "must be between 1 and 50");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.tool_call_max_retries {
        let field = "server.tool_call_max_retries";
        if val > 10 {
            result.add_error(field, "must be between 0 and 10");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.tool_call_retry_delay_ms {
        let field = "server.tool_call_retry_delay_ms";
        if val > 10000 {
            result.add_error(field, "must be between 0 and 10000");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.max_plan_subtasks {
        let field = "server.max_plan_subtasks";
        if !(1..=50).contains(&val) {
            result.add_error(field, "must be between 1 and 50");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.plan_timeout_secs {
        let field = "server.plan_timeout_secs";
        if !(60..=7200).contains(&val) {
            result.add_error(field, "must be between 60 and 7200");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.subtask_max_retries {
        let field = "server.subtask_max_retries";
        if val > 10 {
            result.add_error(field, "must be between 0 and 10");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.subtask_react_max_iterations {
        let field = "server.subtask_react_max_iterations";
        if !(1..=20).contains(&val) {
            result.add_error(field, "must be between 1 and 20");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = server.subtask_react_timeout_secs {
        let field = "server.subtask_react_timeout_secs";
        if !(10..=600).contains(&val) {
            result.add_error(field, "must be between 10 and 600");
        } else {
            result.add_valid(field);
        }
    }
}

/// Validate chat configuration updates
fn validate_chat_config(chat: &ChatConfigUpdate, result: &mut ValidationResult) {
    if let Some(ref url) = chat.url {
        let field = "chat.url";
        if url.is_empty() {
            result.add_error(field, "URL cannot be empty");
        } else if !url.starts_with("http://") && !url.starts_with("https://") {
            result.add_error(field, "URL must start with http:// or https://");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(ref api_key) = chat.api_key {
        let field = "chat.api_key";
        // API key can be empty (to clear it) or non-empty
        if api_key.is_empty() {
            result.add_valid(field); // Allow clearing API key
        } else {
            result.add_valid(field);
        }
    }
}

/// Validate embedding configuration updates
fn validate_embedding_config(embedding: &EmbeddingConfigUpdate, result: &mut ValidationResult) {
    if let Some(ref url) = embedding.url {
        let field = "embedding.url";
        if url.is_empty() {
            result.add_error(field, "URL cannot be empty");
        } else if !url.starts_with("http://") && !url.starts_with("https://") {
            result.add_error(field, "URL must start with http:// or https://");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(ref api_key) = embedding.api_key {
        let field = "embedding.api_key";
        // API key can be empty (to clear it) or non-empty
        if api_key.is_empty() {
            result.add_valid(field); // Allow clearing API key
        } else {
            result.add_valid(field);
        }
    }
}

/// Validate memory configuration updates
fn validate_memory_config(memory: &MemoryConfigUpdate, result: &mut ValidationResult) {
    if memory.auto_summarize.is_some() {
        result.add_valid("memory.auto_summarize");
    }

    if memory.summarization_strategy.is_some() {
        result.add_valid("memory.summarization_strategy");
    }

    if let Some(val) = memory.summarize_threshold {
        let field = "memory.summarize_threshold";
        if !(2..=100).contains(&val) {
            result.add_error(field, "must be between 2 and 100");
        } else {
            result.add_valid(field);
        }
    }

    if let Some(val) = memory.max_stored_messages {
        let field = "memory.max_stored_messages";
        if !(5..=500).contains(&val) {
            result.add_error(field, "must be between 5 and 500");
        } else {
            result.add_valid(field);
        }
    }
}

/// Validate RAG configuration updates
fn validate_rag_config(rag: &RagConfigUpdate, result: &mut ValidationResult) {
    if rag.enable.is_some() {
        result.add_valid("rag.enable");
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_field_updatable() {
        assert!(is_field_updatable("server.max_tools_per_iteration"));
        assert!(is_field_updatable("chat.url"));
        assert!(is_field_updatable("chat.api_key"));
        assert!(!is_field_updatable("server.host"));
        assert!(!is_field_updatable("server.port"));
    }

    #[test]
    fn test_validate_server_config_valid() {
        let server = ServerConfigUpdate {
            max_tools_per_iteration: Some(10),
            tool_call_max_retries: Some(3),
            tool_call_retry_delay_ms: Some(1000),
            max_plan_subtasks: Some(20),
            plan_timeout_secs: Some(600),
            subtask_max_retries: Some(2),
            subtask_react_max_iterations: Some(5),
            subtask_react_timeout_secs: Some(60),
        };

        let mut result = ValidationResult::default();
        validate_server_config(&server, &mut result);

        assert!(result.is_valid());
        assert_eq!(result.valid_fields.len(), 8);
    }

    #[test]
    fn test_validate_server_config_invalid() {
        let server = ServerConfigUpdate {
            max_tools_per_iteration: Some(100), // Too high
            tool_call_max_retries: Some(20),    // Too high
            tool_call_retry_delay_ms: None,
            max_plan_subtasks: Some(0),  // Too low
            plan_timeout_secs: Some(10), // Too low
            subtask_max_retries: None,
            subtask_react_max_iterations: None,
            subtask_react_timeout_secs: None,
        };

        let mut result = ValidationResult::default();
        validate_server_config(&server, &mut result);

        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 4);
    }

    #[test]
    fn test_validate_chat_config_valid_url() {
        let chat = ChatConfigUpdate {
            url: Some("http://localhost:8080/v1".to_string()),
            api_key: Some("sk-test-key".to_string()),
        };

        let mut result = ValidationResult::default();
        validate_chat_config(&chat, &mut result);

        assert!(result.is_valid());
        assert_eq!(result.valid_fields.len(), 2);
    }

    #[test]
    fn test_validate_chat_config_invalid_url() {
        let chat = ChatConfigUpdate {
            url: Some("invalid-url".to_string()),
            api_key: None,
        };

        let mut result = ValidationResult::default();
        validate_chat_config(&chat, &mut result);

        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].message.contains("http://"));
    }

    #[test]
    fn test_validate_memory_config_valid() {
        let memory = MemoryConfigUpdate {
            auto_summarize: Some(true),
            summarization_strategy: None,
            summarize_threshold: Some(12),
            max_stored_messages: Some(20),
        };

        let mut result = ValidationResult::default();
        validate_memory_config(&memory, &mut result);

        assert!(result.is_valid());
        assert_eq!(result.valid_fields.len(), 3);
    }

    #[test]
    fn test_validate_memory_config_invalid() {
        let memory = MemoryConfigUpdate {
            auto_summarize: None,
            summarization_strategy: None,
            summarize_threshold: Some(1),    // Too low
            max_stored_messages: Some(1000), // Too high
        };

        let mut result = ValidationResult::default();
        validate_memory_config(&memory, &mut result);

        assert!(!result.is_valid());
        assert_eq!(result.errors.len(), 2);
    }

    #[test]
    fn test_validate_full_request() {
        let request = ConfigUpdateRequest {
            server: Some(ServerConfigUpdate {
                max_tools_per_iteration: Some(10),
                tool_call_max_retries: None,
                tool_call_retry_delay_ms: None,
                max_plan_subtasks: None,
                plan_timeout_secs: None,
                subtask_max_retries: None,
                subtask_react_max_iterations: None,
                subtask_react_timeout_secs: None,
            }),
            chat: Some(ChatConfigUpdate {
                url: Some("https://api.example.com/v1".to_string()),
                api_key: None,
            }),
            embedding: None,
            memory: None,
            rag: Some(RagConfigUpdate { enable: Some(true) }),
        };

        let result = validate_config_update(&request);

        assert!(result.is_valid());
        assert_eq!(result.valid_fields.len(), 3);
    }
}
