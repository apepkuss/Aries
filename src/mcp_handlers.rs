//! HTTP handlers for MCP Tools management API
//!
//! Provides RESTful endpoints for listing MCP tools:
//! - List all MCP tools from configured servers

use std::sync::Arc;

use axum::{Json, extract::State, http::HeaderMap};
use serde::Serialize;

use crate::{AppState, dual_info, mcp::format_mcp_tool_name};

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
