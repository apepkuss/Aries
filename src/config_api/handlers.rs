//! Configuration API handlers
//!
//! This module contains HTTP handlers for configuration management endpoints.

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};

use super::{
    sanitize::Sanitize,
    types::{
        ConfigSchemaResponse, ConfigSchemaSection, ConfigUpdateRequest, ConfigUpdateResponse,
        FieldSchema, SanitizedConfig,
    },
    update::apply_config_update,
    validate::validate_config_update,
};
use crate::{AppState, dual_info, dual_warn};

// ============================================================================
// GET /v1/config - Get current configuration
// ============================================================================

/// GET /v1/config - Get current configuration (sanitized)
///
/// Returns the current server configuration with sensitive fields redacted.
/// API keys are replaced with boolean flags indicating whether they are configured.
///
/// # Response
///
/// Returns a `SanitizedConfig` JSON object containing:
/// - All configuration sections with sensitive fields redacted
/// - A list of fields that can be updated at runtime
///
/// # Example Response
///
/// ```json
/// {
///   "server": {
///     "host": "127.0.0.1",
///     "port": 3389,
///     "max_tools_per_iteration": 5
///   },
///   "chat": {
///     "url": "http://localhost:8080/v1",
///     "api_key_configured": true
///   },
///   "updatable_fields": ["server.max_tools_per_iteration", "chat.url"]
/// }
/// ```
pub async fn get_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Json<SanitizedConfig> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    dual_info!("Getting configuration - request_id: {}", request_id);

    // Read the current configuration
    let config = state.config.read().await;

    // Convert to sanitized version
    let sanitized = config.sanitize();

    dual_info!(
        "Configuration retrieved successfully - request_id: {}",
        request_id
    );

    Json(sanitized)
}

// ============================================================================
// GET /v1/config/schema - Get configuration schema
// ============================================================================

/// GET /v1/config/schema - Get updatable fields schema
///
/// Returns JSON Schema definitions for all fields that can be updated at runtime,
/// including type information, constraints, and descriptions.
///
/// # Response
///
/// Returns a `ConfigSchemaResponse` JSON object containing:
/// - `updatable`: Schema definitions for updatable fields
/// - `readonly`: List of read-only field paths
///
/// # Example Response
///
/// ```json
/// {
///   "updatable": {
///     "server": {
///       "max_tools_per_iteration": {
///         "type": "integer",
///         "minimum": 1,
///         "maximum": 20,
///         "default": 5,
///         "description": "Maximum number of tool calls per iteration"
///       }
///     }
///   },
///   "readonly": ["server.host", "server.port"]
/// }
/// ```
pub async fn get_config_schema_handler(headers: HeaderMap) -> Json<ConfigSchemaResponse> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    dual_info!("Getting configuration schema - request_id: {}", request_id);

    let schema = build_config_schema();

    dual_info!(
        "Configuration schema retrieved successfully - request_id: {}",
        request_id
    );

    Json(schema)
}

// ============================================================================
// POST /v1/config - Update configuration
// ============================================================================

/// POST /v1/config - Update configuration fields
///
/// Updates specified configuration fields at runtime. Only fields listed in
/// `updatable_fields` can be modified. Some fields may trigger side effects
/// like service reloading.
///
/// # Request Body
///
/// A `ConfigUpdateRequest` JSON object containing the fields to update.
/// Only include the fields you want to modify.
///
/// # Response
///
/// Returns a `ConfigUpdateResponse` JSON object containing:
/// - `success`: Whether all requested updates succeeded
/// - `updated_fields`: List of fields that were successfully updated
/// - `failed_fields`: Map of fields that failed with error messages
/// - `requires_action`: Side effects triggered by the updates
///
/// # Example Request
///
/// ```json
/// {
///   "server": {
///     "max_tools_per_iteration": 10
///   },
///   "memory": {
///     "auto_summarize": false
///   }
/// }
/// ```
///
/// # Example Response
///
/// ```json
/// {
///   "success": true,
///   "updated_fields": ["server.max_tools_per_iteration", "memory.auto_summarize"],
///   "message": "Configuration updated successfully: 2 field(s)"
/// }
/// ```
pub async fn update_config_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(request): Json<ConfigUpdateRequest>,
) -> impl IntoResponse {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    dual_info!("Updating configuration - request_id: {}", request_id);

    // Step 1: Validate the update request
    let validation_result = validate_config_update(&request);

    if !validation_result.errors.is_empty() {
        dual_warn!(
            "Configuration update validation failed - request_id: {} - errors: {:?}",
            request_id,
            validation_result.errors
        );

        let mut failed_fields = HashMap::new();
        for error in validation_result.errors {
            failed_fields.insert(error.field, error.message);
        }

        let response = ConfigUpdateResponse::partial(Vec::new(), failed_fields);
        return (StatusCode::BAD_REQUEST, Json(response));
    }

    // Step 2: Apply the updates
    let mut config = state.config.write().await;
    let update_result = apply_config_update(&mut config, &request, &validation_result.valid_fields);

    // Step 3: Build response
    let response = update_result.into_response();

    let status = if response.success {
        dual_info!(
            "Configuration updated successfully - request_id: {} - fields: {:?}",
            request_id,
            response.updated_fields
        );
        StatusCode::OK
    } else {
        dual_warn!(
            "Configuration update partially failed - request_id: {} - updated: {:?}, failed: {:?}",
            request_id,
            response.updated_fields,
            response.failed_fields
        );
        StatusCode::PARTIAL_CONTENT
    };

    (status, Json(response))
}

/// Build the configuration schema with all field definitions
fn build_config_schema() -> ConfigSchemaResponse {
    // Build server fields schema
    let mut server_fields = HashMap::new();

    server_fields.insert(
        "max_tools_per_iteration".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(1),
            maximum: Some(50),
            default: Some(serde_json::json!(5)),
            description: "Maximum number of tool calls allowed per iteration".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "tool_call_max_retries".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(0),
            maximum: Some(10),
            default: Some(serde_json::json!(2)),
            description: "Maximum number of retries for failed tool calls".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "tool_call_retry_delay_ms".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(0),
            maximum: Some(10000),
            default: Some(serde_json::json!(500)),
            description: "Delay in milliseconds between tool call retries".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "max_plan_subtasks".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(1),
            maximum: Some(50),
            default: Some(serde_json::json!(10)),
            description: "Maximum number of subtasks allowed in a plan".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "plan_timeout_secs".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(60),
            maximum: Some(7200),
            default: Some(serde_json::json!(600)),
            description: "Timeout in seconds for the entire plan execution".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "subtask_max_retries".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(0),
            maximum: Some(10),
            default: Some(serde_json::json!(2)),
            description: "Maximum retries for failed subtasks".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "subtask_react_max_iterations".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(1),
            maximum: Some(20),
            default: Some(serde_json::json!(5)),
            description: "Maximum React iterations per subtask".to_string(),
            side_effect: None,
        },
    );

    server_fields.insert(
        "subtask_react_timeout_secs".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(10),
            maximum: Some(600),
            default: Some(serde_json::json!(60)),
            description: "Timeout in seconds for each subtask's React loop".to_string(),
            side_effect: None,
        },
    );

    // Build chat fields schema
    let mut chat_fields = HashMap::new();

    chat_fields.insert(
        "url".to_string(),
        FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: None,
            description: "Chat service URL".to_string(),
            side_effect: Some("re_register_downstream_server".to_string()),
        },
    );

    chat_fields.insert(
        "api_key".to_string(),
        FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: None,
            description: "Chat service API key".to_string(),
            side_effect: Some("re_register_downstream_server".to_string()),
        },
    );

    // Build embedding fields schema
    let mut embedding_fields = HashMap::new();

    embedding_fields.insert(
        "url".to_string(),
        FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: None,
            description: "Embedding service URL".to_string(),
            side_effect: Some("re_register_downstream_server".to_string()),
        },
    );

    embedding_fields.insert(
        "api_key".to_string(),
        FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: None,
            description: "Embedding service API key".to_string(),
            side_effect: Some("re_register_downstream_server".to_string()),
        },
    );

    // Build memory fields schema
    let mut memory_fields = HashMap::new();

    memory_fields.insert(
        "auto_summarize".to_string(),
        FieldSchema {
            field_type: "boolean".to_string(),
            minimum: None,
            maximum: None,
            default: Some(serde_json::json!(true)),
            description: "Enable automatic message summarization".to_string(),
            side_effect: None,
        },
    );

    memory_fields.insert(
        "summarization_strategy".to_string(),
        FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: Some(serde_json::json!("Incremental")),
            description: "Summarization strategy: Incremental or FullHistory".to_string(),
            side_effect: None,
        },
    );

    memory_fields.insert(
        "summarize_threshold".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(2),
            maximum: Some(100),
            default: Some(serde_json::json!(12)),
            description: "Base number for calculating minimum messages to keep".to_string(),
            side_effect: None,
        },
    );

    memory_fields.insert(
        "max_stored_messages".to_string(),
        FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(5),
            maximum: Some(500),
            default: Some(serde_json::json!(20)),
            description: "Maximum messages before triggering summarization".to_string(),
            side_effect: None,
        },
    );

    // Build rag fields schema
    let mut rag_fields = HashMap::new();

    rag_fields.insert(
        "enable".to_string(),
        FieldSchema {
            field_type: "boolean".to_string(),
            minimum: None,
            maximum: None,
            default: Some(serde_json::json!(false)),
            description: "Enable or disable RAG functionality".to_string(),
            side_effect: None,
        },
    );

    // Build readonly fields list
    let readonly = vec![
        "server.host".to_string(),
        "server.port".to_string(),
        "memory.enable".to_string(),
        "memory.database_path".to_string(),
        "memory.context_window".to_string(),
        "memory.summary_service_base_url".to_string(),
        "memory.summary_service_api_key".to_string(),
        "rag.policy".to_string(),
        "rag.context_window".to_string(),
        "artifacts.enabled".to_string(),
        "artifacts.database_path".to_string(),
        "artifacts.storage_path".to_string(),
        "skill.enabled".to_string(),
        "skill.directories".to_string(),
        "mcp.*".to_string(),
    ];

    ConfigSchemaResponse {
        updatable: ConfigSchemaSection {
            server: Some(server_fields),
            chat: Some(chat_fields),
            embedding: Some(embedding_fields),
            memory: Some(memory_fields),
            rag: Some(rag_fields),
        },
        readonly,
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_api::validate::is_field_updatable;

    #[test]
    fn test_is_field_updatable() {
        assert!(is_field_updatable("server.max_tools_per_iteration"));
        assert!(is_field_updatable("chat.url"));
        assert!(is_field_updatable("chat.api_key"));
        assert!(!is_field_updatable("server.host"));
        assert!(!is_field_updatable("server.port"));
        assert!(!is_field_updatable("memory.database_path"));
    }

    #[test]
    fn test_build_config_schema() {
        let schema = build_config_schema();

        // Check server fields exist
        assert!(schema.updatable.server.is_some());
        let server = schema.updatable.server.unwrap();
        assert!(server.contains_key("max_tools_per_iteration"));
        assert!(server.contains_key("plan_timeout_secs"));

        // Check chat fields exist
        assert!(schema.updatable.chat.is_some());
        let chat = schema.updatable.chat.unwrap();
        assert!(chat.contains_key("url"));
        assert!(chat.contains_key("api_key"));
        // Check side effect is set
        assert_eq!(
            chat.get("url").unwrap().side_effect,
            Some("re_register_downstream_server".to_string())
        );

        // Check readonly fields
        assert!(schema.readonly.contains(&"server.host".to_string()));
        assert!(schema.readonly.contains(&"server.port".to_string()));
    }

    #[test]
    fn test_field_schema_serialization() {
        let field = FieldSchema {
            field_type: "integer".to_string(),
            minimum: Some(1),
            maximum: Some(100),
            default: Some(serde_json::json!(5)),
            description: "Test field".to_string(),
            side_effect: None,
        };

        let json = serde_json::to_string(&field).unwrap();
        assert!(json.contains("\"type\":\"integer\""));
        assert!(json.contains("\"minimum\":1"));
        assert!(json.contains("\"maximum\":100"));
        assert!(json.contains("\"default\":5"));
        assert!(!json.contains("side_effect"));
    }

    #[test]
    fn test_field_schema_with_side_effect() {
        let field = FieldSchema {
            field_type: "string".to_string(),
            minimum: None,
            maximum: None,
            default: None,
            description: "URL field".to_string(),
            side_effect: Some("reload_service".to_string()),
        };

        let json = serde_json::to_string(&field).unwrap();
        assert!(json.contains("\"side_effect\":\"reload_service\""));
    }
}
