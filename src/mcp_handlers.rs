//! HTTP handlers for MCP Tools and Servers management API
//!
//! Provides RESTful endpoints for:
//! - List all MCP tools from configured servers
//! - List all MCP servers with sanitized configuration
//! - Toggle (enable/disable) individual MCP servers at runtime
//! - Update API keys for MCP servers

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
};
use endpoints::chat::McpTransport;
use serde::{Deserialize, Serialize};

use crate::{
    AppState,
    config_api::{persist::persist_config, sanitize::Sanitize},
    dual_error, dual_info,
    mcp::{MCP_SERVICES, format_mcp_tool_name},
};

// ============================================================================
// Response Types
// ============================================================================

/// Information about an MCP tool
#[derive(Debug, Clone, Serialize)]
pub struct McpToolInfo {
    /// Full tool name in mcp__{server}__{tool} format
    pub name: String,
    /// Original tool name without prefix
    pub display_name: String,
    /// MCP server name that provides this tool
    pub server_name: String,
    /// Tool description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the tool is enabled
    pub enabled: bool,
    /// Tool parameters JSON Schema
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// Response for listing MCP tools
#[derive(Debug, Serialize)]
pub struct McpToolListResponse {
    /// List of MCP tools
    pub tools: Vec<McpToolInfo>,
    /// Total count of tools
    pub total: usize,
    /// Number of enabled tools
    pub enabled_count: usize,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /api/mcp/tools - List all MCP tools
///
/// Returns a list of all configured MCP tools from all servers.
pub async fn list_mcp_tools_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Json<McpToolListResponse> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    dual_info!("Listing all MCP tools - request_id: {}", request_id);

    let mut tools = Vec::new();
    let mut enabled_count = 0;

    // Read MCP configuration
    let config = state.config.read().await;
    if let Some(mcp_config) = config.mcp.as_ref() {
        for server_config in mcp_config.server.tool_servers.iter() {
            let server_name = server_config
                .server_name
                .as_deref()
                .unwrap_or(&server_config.name);

            if let Some(server_tools) = server_config.tools.as_ref() {
                for mcp_tool in server_tools.iter() {
                    let full_name = format_mcp_tool_name(server_name, &mcp_tool.name);

                    let tool_info = McpToolInfo {
                        name: full_name,
                        display_name: mcp_tool.name.to_string(),
                        server_name: server_name.to_string(),
                        description: mcp_tool.description.as_ref().map(|s| s.to_string()),
                        enabled: server_config.enable,
                        parameters: Some(serde_json::Value::Object(
                            (*mcp_tool.input_schema).clone(),
                        )),
                    };

                    if server_config.enable {
                        enabled_count += 1;
                    }

                    tools.push(tool_info);
                }
            }
        }
    }

    let total = tools.len();

    dual_info!(
        "Found {} MCP tools ({} enabled) - request_id: {}",
        total,
        enabled_count,
        request_id
    );

    Json(McpToolListResponse {
        tools,
        total,
        enabled_count,
    })
}

// ============================================================================
// MCP Server Management Types
// ============================================================================

use crate::config_api::types::SanitizedMcpToolServerConfig;

/// Response for listing MCP servers
#[derive(Debug, Serialize)]
pub struct McpServerListResponse {
    /// List of sanitized MCP server configurations
    pub servers: Vec<SanitizedMcpToolServerConfig>,
    /// Total count of servers
    pub total: usize,
    /// Number of enabled servers
    pub enabled_count: usize,
}

/// Request to toggle (enable/disable) an MCP server
#[derive(Debug, Deserialize)]
pub struct ToggleMcpServerRequest {
    pub enable: bool,
}

/// Response after toggling an MCP server
#[derive(Debug, Serialize)]
pub struct ToggleMcpServerResponse {
    pub success: bool,
    pub message: String,
    pub server: SanitizedMcpToolServerConfig,
}

/// Request to update an MCP server's API key
#[derive(Debug, Deserialize)]
pub struct UpdateApiKeyRequest {
    pub api_key: String,
    #[serde(default)]
    pub api_key_param: Option<String>,
}

/// Response after updating an MCP server's API key
#[derive(Debug, Serialize)]
pub struct UpdateApiKeyResponse {
    pub success: bool,
    pub message: String,
}

// ============================================================================
// MCP Server Management Handlers
// ============================================================================

/// GET /api/mcp/servers - List all MCP servers
///
/// Returns a sanitized list of all configured MCP servers.
pub async fn list_mcp_servers_handler(
    State(state): State<Arc<AppState>>,
) -> Json<McpServerListResponse> {
    let config = state.config.read().await;
    let mut servers = Vec::new();
    let mut enabled_count = 0;

    if let Some(mcp_config) = config.mcp.as_ref() {
        for server_config in &mcp_config.server.tool_servers {
            let sanitized = server_config.sanitize();
            if sanitized.enable {
                enabled_count += 1;
            }
            servers.push(sanitized);
        }
    }

    let total = servers.len();
    Json(McpServerListResponse {
        servers,
        total,
        enabled_count,
    })
}

/// POST /api/mcp/servers/{name}/toggle - Enable or disable an MCP server
///
/// Enables or disables an MCP server at runtime. When enabling, the server
/// connection is established. When disabling, the connection is torn down.
/// Changes are persisted to the config file.
pub async fn toggle_mcp_server_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<ToggleMcpServerRequest>,
) -> impl IntoResponse {
    dual_info!("Toggle MCP server '{}' -> enable={}", name, body.enable);

    // Find the server and check current state
    {
        let config = state.config.read().await;
        let mcp_config = match config.mcp.as_ref() {
            Some(mcp) => mcp,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ToggleMcpServerResponse {
                        success: false,
                        message: "MCP configuration not found".to_string(),
                        server: SanitizedMcpToolServerConfig {
                            name: name.clone(),
                            transport: String::new(),
                            url: None,
                            command: None,
                            enable: false,
                            tools_count: 0,
                            api_key_configured: false,
                            api_key_param: None,
                        },
                    }),
                );
            }
        };

        let server = mcp_config
            .server
            .tool_servers
            .iter()
            .find(|s| s.name == name);
        match server {
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(ToggleMcpServerResponse {
                        success: false,
                        message: format!("MCP server '{}' not found", name),
                        server: SanitizedMcpToolServerConfig {
                            name: name.clone(),
                            transport: String::new(),
                            url: None,
                            command: None,
                            enable: false,
                            tools_count: 0,
                            api_key_configured: false,
                            api_key_param: None,
                        },
                    }),
                );
            }
            Some(s) if s.enable == body.enable => {
                let status = if body.enable { "enabled" } else { "disabled" };
                return (
                    StatusCode::OK,
                    Json(ToggleMcpServerResponse {
                        success: true,
                        message: format!("MCP server '{}' is already {}", name, status),
                        server: s.sanitize(),
                    }),
                );
            }
            _ => {} // State change needed, continue below
        }
    }

    if body.enable {
        // === Enable flow ===
        // 1. Clone the server config
        let mut server_clone = {
            let config = state.config.read().await;
            let mcp_config = config.mcp.as_ref().unwrap();
            let server = mcp_config
                .server
                .tool_servers
                .iter()
                .find(|s| s.name == name)
                .unwrap();
            server.clone()
        };

        // 2. Set enable = true and attempt connection
        server_clone.enable = true;
        if let Err(e) = server_clone.connect_mcp_server().await {
            dual_error!("Failed to enable MCP server '{}': {}", name, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ToggleMcpServerResponse {
                    success: false,
                    message: format!("Failed to connect MCP server '{}': {}", name, e),
                    server: server_clone.sanitize(),
                }),
            );
        }

        // 3. Connection succeeded - update config
        {
            let mut config = state.config.write().await;
            if let Some(mcp_config) = config.mcp.as_mut()
                && let Some(server) = mcp_config
                    .server
                    .tool_servers
                    .iter_mut()
                    .find(|s| s.name == name)
            {
                // Update with the connected clone (has tools populated)
                *server = server_clone.clone();
            }
        }

        // 4. Persist config
        persist_and_record(&state).await;

        let sanitized = server_clone.sanitize();
        dual_info!("MCP server '{}' enabled successfully", name);
        (
            StatusCode::OK,
            Json(ToggleMcpServerResponse {
                success: true,
                message: format!("MCP server '{}' enabled successfully", name),
                server: sanitized,
            }),
        )
    } else {
        // === Disable flow ===
        let (transport, service_name) = {
            let config = state.config.read().await;
            let mcp_config = config.mcp.as_ref().unwrap();
            let server = mcp_config
                .server
                .tool_servers
                .iter()
                .find(|s| s.name == name)
                .unwrap();
            let svc_name = server
                .server_name
                .clone()
                .unwrap_or_else(|| server.name.clone());
            (server.transport, svc_name)
        };

        // 1. Update config first
        let sanitized = {
            let mut config = state.config.write().await;
            let mcp_config = config.mcp.as_mut().unwrap();
            let server = mcp_config
                .server
                .tool_servers
                .iter_mut()
                .find(|s| s.name == name)
                .unwrap();
            server.enable = false;
            server.tools = None;
            server.server_name = None;
            server.sanitize()
        };

        // 2. Tear down connection
        match transport {
            McpTransport::Stdio => {
                if let Err(e) = state.stdio_manager.stop_process(&name).await {
                    dual_error!("Failed to stop stdio process '{}': {}", name, e);
                    // Non-fatal: config is already updated
                }
            }
            McpTransport::Sse | McpTransport::StreamHttp => {
                // Remove from MCP_SERVICES (dropping the service closes the connection)
                if let Some(services) = MCP_SERVICES.get() {
                    let mut services = services.write().await;
                    if services.remove(&service_name).is_some() {
                        dual_info!("Removed MCP service '{}' from MCP_SERVICES", service_name);
                    }
                }
            }
        }

        // 3. Persist config
        persist_and_record(&state).await;

        dual_info!("MCP server '{}' disabled successfully", name);
        (
            StatusCode::OK,
            Json(ToggleMcpServerResponse {
                success: true,
                message: format!("MCP server '{}' disabled successfully", name),
                server: sanitized,
            }),
        )
    }
}

/// POST /api/mcp/servers/{name}/api-key - Update API key for an MCP server
///
/// Updates the API key (and optionally the parameter name) for an MCP server.
/// If the server is currently enabled, it will be reconnected with the new key.
/// Changes are persisted to the config file.
pub async fn update_api_key_handler(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(body): Json<UpdateApiKeyRequest>,
) -> impl IntoResponse {
    dual_info!("Updating API key for MCP server '{}'", name);

    // Find the server and update the key
    let (was_enabled, transport, service_name) = {
        let mut config = state.config.write().await;
        let mcp_config = match config.mcp.as_mut() {
            Some(mcp) => mcp,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(UpdateApiKeyResponse {
                        success: false,
                        message: "MCP configuration not found".to_string(),
                    }),
                );
            }
        };

        let server = match mcp_config
            .server
            .tool_servers
            .iter_mut()
            .find(|s| s.name == name)
        {
            Some(s) => s,
            None => {
                return (
                    StatusCode::NOT_FOUND,
                    Json(UpdateApiKeyResponse {
                        success: false,
                        message: format!("MCP server '{}' not found", name),
                    }),
                );
            }
        };

        // Update the API key fields
        let was_enabled = server.enable;
        let transport = server.transport;
        let svc_name = server
            .server_name
            .clone()
            .unwrap_or_else(|| server.name.clone());

        server.api_key = if body.api_key.is_empty() {
            None
        } else {
            Some(body.api_key.clone())
        };
        if let Some(param) = body.api_key_param {
            server.api_key_param = if param.is_empty() { None } else { Some(param) };
        }

        (was_enabled, transport, svc_name)
    };

    // If server is enabled, reconnect with new key
    if was_enabled {
        // Disconnect old connection
        match transport {
            McpTransport::Stdio => {
                // Stdio doesn't use API key in URL, just persist
            }
            McpTransport::Sse | McpTransport::StreamHttp => {
                if let Some(services) = MCP_SERVICES.get() {
                    let mut services = services.write().await;
                    services.remove(&service_name);
                }
            }
        }

        // Reconnect with updated config
        if transport != McpTransport::Stdio {
            let mut server_clone = {
                let config = state.config.read().await;
                let mcp_config = config.mcp.as_ref().unwrap();
                mcp_config
                    .server
                    .tool_servers
                    .iter()
                    .find(|s| s.name == name)
                    .unwrap()
                    .clone()
            };

            if let Err(e) = server_clone.connect_mcp_server().await {
                dual_error!(
                    "Failed to reconnect MCP server '{}' after API key update: {}",
                    name,
                    e
                );
                // Persist the key change anyway
                persist_and_record(&state).await;
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(UpdateApiKeyResponse {
                        success: false,
                        message: format!(
                            "API key updated but reconnection failed: {}. Server has been disabled.",
                            e
                        ),
                    }),
                );
            }

            // Update config with reconnected server (tools refreshed)
            {
                let mut config = state.config.write().await;
                if let Some(mcp_config) = config.mcp.as_mut()
                    && let Some(server) = mcp_config
                        .server
                        .tool_servers
                        .iter_mut()
                        .find(|s| s.name == name)
                {
                    *server = server_clone;
                }
            }
        }
    }

    // Persist config
    persist_and_record(&state).await;

    dual_info!("API key updated for MCP server '{}'", name);
    (
        StatusCode::OK,
        Json(UpdateApiKeyResponse {
            success: true,
            message: format!("API key updated for MCP server '{}'", name),
        }),
    )
}

/// Helper: persist config and record update time
async fn persist_and_record(state: &AppState) {
    if let Some(ref config_path) = state.config_path {
        let config = state.config.read().await;
        let result = persist_config(&config, config_path).await;
        if !result.success {
            dual_error!(
                "Failed to persist config: {}",
                result.error.unwrap_or_default()
            );
        }
    }
    state.record_config_update_time().await;
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_tool_info_serialization() {
        let tool = McpToolInfo {
            name: "mcp__weather-api__get_weather".to_string(),
            display_name: "get_weather".to_string(),
            server_name: "weather-api".to_string(),
            description: Some("Get current weather for a city".to_string()),
            enabled: true,
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            })),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("mcp__weather-api__get_weather"));
        assert!(json.contains("get_weather"));
        assert!(json.contains("weather-api"));
        assert!(json.contains("Get current weather"));
        assert!(json.contains("\"enabled\":true"));
    }

    #[test]
    fn test_mcp_tool_info_without_optional_fields() {
        let tool = McpToolInfo {
            name: "mcp__server__tool".to_string(),
            display_name: "tool".to_string(),
            server_name: "server".to_string(),
            description: None,
            enabled: false,
            parameters: None,
        };

        let json = serde_json::to_string(&tool).unwrap();
        // Optional fields should be omitted when None
        assert!(!json.contains("description"));
        assert!(!json.contains("parameters"));
        assert!(json.contains("\"enabled\":false"));
    }

    #[test]
    fn test_mcp_tool_list_response_serialization() {
        let response = McpToolListResponse {
            tools: vec![
                McpToolInfo {
                    name: "mcp__server1__tool1".to_string(),
                    display_name: "tool1".to_string(),
                    server_name: "server1".to_string(),
                    description: Some("Tool 1".to_string()),
                    enabled: true,
                    parameters: None,
                },
                McpToolInfo {
                    name: "mcp__server2__tool2".to_string(),
                    display_name: "tool2".to_string(),
                    server_name: "server2".to_string(),
                    description: None,
                    enabled: false,
                    parameters: None,
                },
            ],
            total: 2,
            enabled_count: 1,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"total\":2"));
        assert!(json.contains("\"enabled_count\":1"));
        assert!(json.contains("server1"));
        assert!(json.contains("server2"));
    }

    #[test]
    fn test_empty_mcp_tool_list_response() {
        let response = McpToolListResponse {
            tools: vec![],
            total: 0,
            enabled_count: 0,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"tools\":[]"));
        assert!(json.contains("\"total\":0"));
        assert!(json.contains("\"enabled_count\":0"));
    }
}
