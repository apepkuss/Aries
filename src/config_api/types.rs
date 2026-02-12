//! Configuration API type definitions
//!
//! This module contains all request/response types for the configuration API.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::SummarizationStrategy;

// ============================================================================
// Updatable Fields List
// ============================================================================

/// List of all fields that can be updated at runtime
pub const UPDATABLE_FIELDS: &[&str] = &[
    // Server config - simple hot update
    "server.max_tools_per_iteration",
    "server.tool_call_max_retries",
    "server.tool_call_retry_delay_ms",
    "server.max_plan_subtasks",
    "server.plan_timeout_secs",
    "server.subtask_max_retries",
    "server.subtask_react_max_iterations",
    "server.subtask_react_timeout_secs",
    // Embedding config - requires service reload
    "embedding.url",
    "embedding.api_key",
    // Memory config - simple hot update
    "memory.auto_summarize",
    "memory.summarization_strategy",
    "memory.summarize_threshold",
    "memory.max_stored_messages",
    // RAG config - simple hot update
    "rag.enable",
    // Subagent config - simple hot update
    "subagent.execution_mode",
    // HITL config - simple hot update
    "hitl.enabled",
    "hitl.default_timeout_secs",
    "hitl.default_timeout_behavior",
    "hitl.confirmation_threshold",
];

/// Fields that require service reload after update
#[allow(dead_code)]
pub const SIDE_EFFECT_FIELDS: &[&str] = &["embedding.url", "embedding.api_key"];

// ============================================================================
// Sanitized Config Response Types
// ============================================================================

/// Sanitized configuration response
///
/// Contains the full configuration with sensitive fields redacted.
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedConfig {
    /// Server configuration
    pub server: SanitizedServerConfig,

    /// Chat service configuration (sanitized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<SanitizedChatConfig>,

    /// Embedding service configuration (sanitized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<SanitizedEmbeddingConfig>,

    /// Memory system configuration (sanitized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<SanitizedMemoryConfig>,

    /// RAG configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag: Option<SanitizedRagConfig>,

    /// MCP configuration (sanitized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp: Option<SanitizedMcpConfig>,

    /// Skills configuration (sanitized)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<SanitizedSkillConfig>,

    /// Artifacts configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<SanitizedArtifactsConfig>,

    /// Sub-Agent configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<SanitizedSubagentConfig>,

    /// HITL (Human-in-the-Loop) configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitl: Option<SanitizedHitlConfig>,

    /// Session history configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<SanitizedSessionConfig>,

    /// List of fields that can be updated at runtime
    pub updatable_fields: Vec<String>,
}

/// Sanitized server configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedServerConfig {
    pub host: String,
    pub port: u16,
    pub max_tools_per_iteration: usize,
    pub tool_call_max_retries: u32,
    pub tool_call_retry_delay_ms: u64,
    pub max_plan_subtasks: usize,
    pub plan_timeout_secs: u64,
    pub subtask_max_retries: u32,
    pub subtask_react_max_iterations: u32,
    pub subtask_react_timeout_secs: u64,
}

/// Sanitized chat service configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedChatConfig {
    /// Chat service URL
    pub url: String,
    /// Whether an API key is configured (actual key is hidden)
    pub api_key_configured: bool,
    /// Model used for chat completions
    pub model: String,
}

/// Sanitized embedding service configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedEmbeddingConfig {
    /// Embedding service URL
    pub url: String,
    /// Whether an API key is configured (actual key is hidden)
    pub api_key_configured: bool,
}

/// Sanitized memory system configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedMemoryConfig {
    pub enable: bool,
    pub database_path: String,
    pub context_window: u64,
    pub auto_summarize: bool,
    pub summarization_strategy: SummarizationStrategy,
    pub summarize_threshold: u32,
    pub max_stored_messages: u32,
    /// Whether a summary service API key is configured
    pub summary_service_api_key_configured: bool,
}

/// Sanitized RAG configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedRagConfig {
    pub enable: bool,
    pub policy: String,
    pub context_window: u64,
}

/// Sanitized MCP configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedMcpConfig {
    pub server: SanitizedMcpServerConfig,
}

/// Sanitized MCP server configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedMcpServerConfig {
    pub tool_servers: Vec<SanitizedMcpToolServerConfig>,
}

/// Sanitized MCP tool server configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedMcpToolServerConfig {
    pub name: String,
    pub transport: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Command for stdio transport
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub enable: bool,
    /// Number of tools (not the full list)
    pub tools_count: usize,
}

/// Sanitized skills configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedSkillConfig {
    pub enabled: bool,
    pub directories: Vec<String>,
    pub max_reference_size: usize,
    /// Whether an API key is configured for skills API
    pub api_key_configured: bool,
    /// Whether a market API key is configured
    pub market_api_key_configured: bool,
}

/// Sanitized artifacts configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedArtifactsConfig {
    pub enabled: bool,
    pub database_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_path: Option<String>,
    pub max_content_size: u64,
    pub max_binary_size: u64,
    pub retention_days: u32,
    pub cleanup_interval_secs: u64,
    pub soft_delete_retention_days: u32,
    pub enable_cleanup: bool,
}

/// Sanitized Sub-Agent configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedSubagentConfig {
    /// Whether Sub-Agent functionality is enabled
    pub enabled: bool,
    /// Execution mode: "direct" (Plan mode) or "subagent" (Sub-Agent mode)
    pub execution_mode: String,
    /// Parallel mode for subtask execution: "auto" | "sequential" | "manual"
    pub parallel_mode: String,
    /// Maximum concurrent Sub-Agents
    pub max_concurrent: usize,
    /// Default timeout per Sub-Agent in seconds
    pub default_timeout_secs: u64,
    /// Default maximum iterations per Sub-Agent
    pub default_max_iterations: u32,
}

/// Sanitized HITL (Human-in-the-Loop) configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedHitlConfig {
    /// Whether HITL is enabled
    pub enabled: bool,
    /// Default timeout in seconds
    pub default_timeout_secs: u64,
    /// Default timeout behavior
    pub default_timeout_behavior: String,
    /// Minimum risk level that requires confirmation
    pub confirmation_threshold: String,
    /// Number of tool overrides configured
    pub tool_overrides_count: usize,
    /// Whether runtime learning is enabled
    pub runtime_learning_enabled: bool,
}

/// Sanitized session history configuration
#[derive(Debug, Clone, Serialize)]
pub struct SanitizedSessionConfig {
    /// Whether session history is enabled
    pub enable: bool,
    /// Storage directory path
    pub storage_path: String,
}

// ============================================================================
// Config Update Request Types
// ============================================================================

/// Configuration update request
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ConfigUpdateRequest {
    /// Server configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerConfigUpdate>,

    /// Chat configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<ChatConfigUpdate>,

    /// Embedding configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<EmbeddingConfigUpdate>,

    /// Memory configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryConfigUpdate>,

    /// RAG configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag: Option<RagConfigUpdate>,

    /// Sub-Agent configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent: Option<SubagentConfigUpdate>,

    /// HITL configuration updates
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hitl: Option<HitlConfigUpdate>,
}

/// Server configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfigUpdate {
    pub max_tools_per_iteration: Option<usize>,
    pub tool_call_max_retries: Option<u32>,
    pub tool_call_retry_delay_ms: Option<u64>,
    pub max_plan_subtasks: Option<usize>,
    pub plan_timeout_secs: Option<u64>,
    pub subtask_max_retries: Option<u32>,
    pub subtask_react_max_iterations: Option<u32>,
    pub subtask_react_timeout_secs: Option<u64>,
}

/// Chat configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct ChatConfigUpdate {
    pub url: Option<String>,
    pub api_key: Option<String>,
    pub model: Option<String>,
}

/// Embedding configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfigUpdate {
    pub url: Option<String>,
    pub api_key: Option<String>,
}

/// Memory configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryConfigUpdate {
    pub auto_summarize: Option<bool>,
    pub summarization_strategy: Option<SummarizationStrategy>,
    pub summarize_threshold: Option<u32>,
    pub max_stored_messages: Option<u32>,
}

/// RAG configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct RagConfigUpdate {
    pub enable: Option<bool>,
}

/// Sub-Agent configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SubagentConfigUpdate {
    /// Execution mode: "direct" (Plan mode) or "subagent" (Sub-Agent mode)
    pub execution_mode: Option<String>,
    /// Parallel mode for subtask execution: "auto" | "sequential" | "manual"
    pub parallel_mode: Option<String>,
}

/// HITL configuration updatable fields
#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct HitlConfigUpdate {
    /// Whether HITL is enabled
    pub enabled: Option<bool>,
    /// Default timeout in seconds
    pub default_timeout_secs: Option<u64>,
    /// Default timeout behavior (reject, approve, skip, abort, wait)
    pub default_timeout_behavior: Option<String>,
    /// Minimum risk level that requires confirmation (low, medium, high, critical)
    pub confirmation_threshold: Option<String>,
}

// ============================================================================
// Config Update Response Types
// ============================================================================

/// Configuration update response
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
pub struct ConfigUpdateResponse {
    /// Whether the update was successful
    pub success: bool,

    /// List of successfully updated fields
    pub updated_fields: Vec<String>,

    /// Fields that failed to update with error messages
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub failed_fields: HashMap<String, String>,

    /// Side effects that were triggered
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub requires_action: HashMap<String, String>,

    /// Human-readable message
    pub message: String,
}

#[allow(dead_code)]
impl ConfigUpdateResponse {
    /// Create a successful response
    pub fn success(updated_fields: Vec<String>) -> Self {
        let message = if updated_fields.is_empty() {
            "No fields were updated".to_string()
        } else {
            format!(
                "Configuration updated successfully: {} field(s)",
                updated_fields.len()
            )
        };

        Self {
            success: true,
            updated_fields,
            failed_fields: HashMap::new(),
            requires_action: HashMap::new(),
            message,
        }
    }

    /// Create a partial success response
    pub fn partial(updated_fields: Vec<String>, failed_fields: HashMap<String, String>) -> Self {
        Self {
            success: false,
            updated_fields,
            failed_fields,
            requires_action: HashMap::new(),
            message: "Some fields could not be updated".to_string(),
        }
    }

    /// Create a failure response
    pub fn failure(message: String) -> Self {
        Self {
            success: false,
            updated_fields: Vec::new(),
            failed_fields: HashMap::new(),
            requires_action: HashMap::new(),
            message,
        }
    }

    /// Add a side effect action
    pub fn with_action(mut self, field: &str, action: &str) -> Self {
        self.requires_action
            .insert(field.to_string(), action.to_string());
        self
    }
}

// ============================================================================
// Config Schema Types
// ============================================================================

/// Configuration schema response
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSchemaResponse {
    /// Schema for updatable fields
    pub updatable: ConfigSchemaSection,

    /// List of read-only fields
    pub readonly: Vec<String>,
}

/// Schema section for a category of fields
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSchemaSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<HashMap<String, FieldSchema>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat: Option<HashMap<String, FieldSchema>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub embedding: Option<HashMap<String, FieldSchema>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory: Option<HashMap<String, FieldSchema>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub rag: Option<HashMap<String, FieldSchema>>,
}

/// Schema definition for a single field
#[derive(Debug, Clone, Serialize)]
pub struct FieldSchema {
    /// JSON Schema type
    #[serde(rename = "type")]
    pub field_type: String,

    /// Minimum value (for numbers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<i64>,

    /// Maximum value (for numbers)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<i64>,

    /// Default value
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,

    /// Field description
    pub description: String,

    /// Side effect triggered by updating this field
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side_effect: Option<String>,
}

// ============================================================================
// Service Test Types
// ============================================================================

/// Request to test chat service connectivity
#[derive(Debug, Clone, Deserialize)]
pub struct TestChatServiceRequest {
    /// The URL to test
    pub url: String,
    /// Optional API key for authentication
    #[serde(default)]
    pub api_key: Option<String>,
}

/// Response from testing chat service connectivity
#[derive(Debug, Clone, Serialize)]
pub struct TestChatServiceResponse {
    /// Whether the connection test succeeded
    pub success: bool,
    /// Error message if test failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// List of available models (if successful)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
}

impl TestChatServiceResponse {
    pub fn success(models: Vec<String>) -> Self {
        Self {
            success: true,
            error: None,
            models: Some(models),
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            error: Some(error),
            models: None,
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
    fn test_sanitized_config_serialization() {
        let config = SanitizedConfig {
            server: SanitizedServerConfig {
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
            },
            chat: Some(SanitizedChatConfig {
                url: "http://localhost:8080/v1".to_string(),
                api_key_configured: true,
                model: "gpt-4".to_string(),
            }),
            embedding: None,
            memory: None,
            rag: None,
            mcp: None,
            skill: None,
            artifacts: None,
            subagent: None,
            hitl: None,
            session: None,
            updatable_fields: vec!["server.max_tools_per_iteration".to_string()],
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"host\":\"127.0.0.1\""));
        assert!(json.contains("\"api_key_configured\":true"));
        assert!(!json.contains("api_key\":\""));
    }

    #[test]
    fn test_config_update_request_deserialization() {
        let json = r#"{
            "server": {
                "max_tools_per_iteration": 10
            },
            "chat": {
                "url": "http://new-server:8080/v1"
            }
        }"#;

        let request: ConfigUpdateRequest = serde_json::from_str(json).unwrap();
        assert!(request.server.is_some());
        assert_eq!(request.server.unwrap().max_tools_per_iteration, Some(10));
        assert!(request.chat.is_some());
        assert_eq!(
            request.chat.unwrap().url,
            Some("http://new-server:8080/v1".to_string())
        );
    }

    #[test]
    fn test_config_update_response_success() {
        let response =
            ConfigUpdateResponse::success(vec!["server.max_tools_per_iteration".to_string()]);
        assert!(response.success);
        assert_eq!(response.updated_fields.len(), 1);
        assert!(response.failed_fields.is_empty());
    }

    #[test]
    fn test_config_update_response_partial() {
        let mut failed = HashMap::new();
        failed.insert(
            "server.host".to_string(),
            "Field is not updatable at runtime".to_string(),
        );

        let response = ConfigUpdateResponse::partial(
            vec!["server.max_tools_per_iteration".to_string()],
            failed,
        );
        assert!(!response.success);
        assert_eq!(response.updated_fields.len(), 1);
        assert_eq!(response.failed_fields.len(), 1);
    }

    #[test]
    fn test_updatable_fields_list() {
        assert!(UPDATABLE_FIELDS.contains(&"server.max_tools_per_iteration"));
        assert!(!UPDATABLE_FIELDS.contains(&"chat.url"));
        assert!(!UPDATABLE_FIELDS.contains(&"chat.api_key"));
        assert!(!UPDATABLE_FIELDS.contains(&"chat.model"));
        assert!(!UPDATABLE_FIELDS.contains(&"server.host"));
        assert!(!UPDATABLE_FIELDS.contains(&"server.port"));
    }

    #[test]
    fn test_side_effect_fields_list() {
        assert!(!SIDE_EFFECT_FIELDS.contains(&"chat.url"));
        assert!(!SIDE_EFFECT_FIELDS.contains(&"chat.api_key"));
        assert!(SIDE_EFFECT_FIELDS.contains(&"embedding.url"));
        assert!(SIDE_EFFECT_FIELDS.contains(&"embedding.api_key"));
        assert!(!SIDE_EFFECT_FIELDS.contains(&"server.max_tools_per_iteration"));
    }
}
