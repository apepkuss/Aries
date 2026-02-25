//! Capability Introspection API
//!
//! Provides a unified endpoint for listing all Agent capabilities,
//! including both Skills and MCP Tools.
//!
//! # Endpoints
//!
//! - `GET /v1/capabilities` - List all available capabilities
//!
//! # Example Response
//!
//! ```json
//! {
//!   "skills": [
//!     {
//!       "name": "code-review",
//!       "description": "Review code for issues",
//!       "enabled": true,
//!       "allowed_tools": ["Read", "Grep"]
//!     }
//!   ],
//!   "tools": [
//!     {
//!       "name": "mcp__weather-api__get_weather",
//!       "display_name": "get_weather",
//!       "server_name": "weather-api",
//!       "description": "Get weather for a city",
//!       "enabled": true,
//!       "parameters": { ... }
//!     }
//!   ],
//!   "summary": {
//!     "total_skills": 1,
//!     "enabled_skills": 1,
//!     "total_tools": 1,
//!     "enabled_tools": 1
//!   }
//! }
//! ```

use std::sync::Arc;

use axum::{Json, extract::State, http::HeaderMap};
use serde::Serialize;

use crate::{
    AppState, dual_info,
    mcp::format_mcp_tool_name,
    mcp_handlers::McpToolInfo,
    skills::{SkillRegistry, SkillSummary},
};

// ============================================================================
// Response Types
// ============================================================================

/// Skill capability information
#[derive(Debug, Clone, Serialize)]
pub struct SkillCapability {
    /// Skill name
    pub name: String,
    /// Skill description
    pub description: String,
    /// Whether the skill is enabled
    pub enabled: bool,
    /// Tools that this skill is allowed to use
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub allowed_tools: Vec<String>,
    /// Input parameters JSON Schema (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

impl From<SkillSummary> for SkillCapability {
    fn from(summary: SkillSummary) -> Self {
        Self {
            name: summary.name,
            description: summary.description,
            enabled: true, // Skills from registry are always enabled
            allowed_tools: summary.allowed_tools,
            parameters: summary.parameters,
        }
    }
}

/// Summary of capabilities
#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesSummary {
    /// Total number of skills
    pub total_skills: usize,
    /// Number of enabled skills
    pub enabled_skills: usize,
    /// Total number of MCP tools
    pub total_tools: usize,
    /// Number of enabled MCP tools
    pub enabled_tools: usize,
}

/// Response for capabilities endpoint
#[derive(Debug, Serialize)]
pub struct CapabilitiesResponse {
    /// List of available skills
    pub skills: Vec<SkillCapability>,
    /// List of available MCP tools
    pub tools: Vec<McpToolInfo>,
    /// Summary statistics
    pub summary: CapabilitiesSummary,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /v1/capabilities - List all capabilities
///
/// Returns a unified list of all available skills and MCP tools.
pub async fn get_capabilities_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Json<CapabilitiesResponse> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    dual_info!("Getting all capabilities - request_id: {}", request_id);

    // Collect skills
    let skills: Vec<SkillCapability> = if let Ok(registry) = SkillRegistry::global() {
        registry
            .get_summaries()
            .await
            .into_iter()
            .map(SkillCapability::from)
            .collect()
    } else {
        Vec::new()
    };

    let enabled_skills = skills.iter().filter(|s| s.enabled).count();

    // Collect MCP tools
    let mut tools = Vec::new();
    let mut enabled_tools = 0;

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
                        enabled_tools += 1;
                    }

                    tools.push(tool_info);
                }
            }
        }
    }

    let summary = CapabilitiesSummary {
        total_skills: skills.len(),
        enabled_skills,
        total_tools: tools.len(),
        enabled_tools,
    };

    dual_info!(
        "Found {} skills ({} enabled), {} tools ({} enabled) - request_id: {}",
        summary.total_skills,
        summary.enabled_skills,
        summary.total_tools,
        summary.enabled_tools,
        request_id
    );

    Json(CapabilitiesResponse {
        skills,
        tools,
        summary,
    })
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_capability_serialization() {
        let skill = SkillCapability {
            name: "code-review".to_string(),
            description: "Review code for issues".to_string(),
            enabled: true,
            allowed_tools: vec!["Read".to_string(), "Grep".to_string()],
            parameters: None,
        };

        let json = serde_json::to_string(&skill).unwrap();
        assert!(json.contains("code-review"));
        assert!(json.contains("Review code"));
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("Read"));
    }

    #[test]
    fn test_skill_capability_empty_allowed_tools() {
        let skill = SkillCapability {
            name: "simple-skill".to_string(),
            description: "A simple skill".to_string(),
            enabled: true,
            allowed_tools: vec![],
            parameters: None,
        };

        let json = serde_json::to_string(&skill).unwrap();
        // allowed_tools should be omitted when empty
        assert!(!json.contains("allowed_tools"));
    }

    #[test]
    fn test_skill_capability_from_summary() {
        let summary = SkillSummary {
            name: "test-skill".to_string(),
            description: "Test description".to_string(),
            allowed_tools: vec!["Bash".to_string()],
            parameters: None,
            enabled: true,
        };

        let capability: SkillCapability = summary.into();
        assert_eq!(capability.name, "test-skill");
        assert_eq!(capability.description, "Test description");
        assert!(capability.enabled);
        assert_eq!(capability.allowed_tools, vec!["Bash"]);
    }

    #[test]
    fn test_capabilities_summary_serialization() {
        let summary = CapabilitiesSummary {
            total_skills: 5,
            enabled_skills: 3,
            total_tools: 10,
            enabled_tools: 8,
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"total_skills\":5"));
        assert!(json.contains("\"enabled_skills\":3"));
        assert!(json.contains("\"total_tools\":10"));
        assert!(json.contains("\"enabled_tools\":8"));
    }

    #[test]
    fn test_capabilities_response_serialization() {
        let response = CapabilitiesResponse {
            skills: vec![SkillCapability {
                name: "skill1".to_string(),
                description: "Skill 1".to_string(),
                enabled: true,
                allowed_tools: vec![],
                parameters: None,
            }],
            tools: vec![McpToolInfo {
                name: "mcp__server__tool".to_string(),
                display_name: "tool".to_string(),
                server_name: "server".to_string(),
                description: Some("A tool".to_string()),
                enabled: true,
                parameters: None,
            }],
            summary: CapabilitiesSummary {
                total_skills: 1,
                enabled_skills: 1,
                total_tools: 1,
                enabled_tools: 1,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"skills\":["));
        assert!(json.contains("\"tools\":["));
        assert!(json.contains("\"summary\":{"));
        assert!(json.contains("skill1"));
        assert!(json.contains("mcp__server__tool"));
    }

    #[test]
    fn test_empty_capabilities_response() {
        let response = CapabilitiesResponse {
            skills: vec![],
            tools: vec![],
            summary: CapabilitiesSummary {
                total_skills: 0,
                enabled_skills: 0,
                total_tools: 0,
                enabled_tools: 0,
            },
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"skills\":[]"));
        assert!(json.contains("\"tools\":[]"));
        assert!(json.contains("\"total_skills\":0"));
    }
}
