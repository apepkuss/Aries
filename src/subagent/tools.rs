//! Sub-Agent 工具定义
//!
//! 此模块定义供 LLM 调用的 Sub-Agent 相关工具。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// ============================================================================
// Tool Name Constants
// ============================================================================

/// Sub-Agent 工具前缀
pub const SUBAGENT_TOOL_PREFIX: &str = "internal";

/// spawn_sub_agent 工具名称
pub const SPAWN_SUB_AGENT_TOOL: &str = "spawn_sub_agent";

/// get_sub_agent_result 工具名称
pub const GET_SUB_AGENT_RESULT_TOOL: &str = "get_sub_agent_result";

/// cancel_sub_agent 工具名称
pub const CANCEL_SUB_AGENT_TOOL: &str = "cancel_sub_agent";

/// 生成完整的内部工具名称
pub fn subagent_tool_name(tool_name: &str) -> String {
    format!("{SUBAGENT_TOOL_PREFIX}__{tool_name}")
}

/// 检查是否为 Sub-Agent 工具
pub fn is_subagent_tool(tool_name: &str) -> bool {
    let full_spawn = subagent_tool_name(SPAWN_SUB_AGENT_TOOL);
    let full_get_result = subagent_tool_name(GET_SUB_AGENT_RESULT_TOOL);
    let full_cancel = subagent_tool_name(CANCEL_SUB_AGENT_TOOL);

    tool_name == full_spawn || tool_name == full_get_result || tool_name == full_cancel
}

/// 解析 Sub-Agent 工具名称，返回工具名称（不含前缀）
pub fn parse_subagent_tool_name(full_name: &str) -> Option<&str> {
    let prefix = format!("{SUBAGENT_TOOL_PREFIX}__");
    let stripped = full_name.strip_prefix(&prefix)?;

    // 验证是否是有效的 Sub-Agent 工具
    if stripped == SPAWN_SUB_AGENT_TOOL
        || stripped == GET_SUB_AGENT_RESULT_TOOL
        || stripped == CANCEL_SUB_AGENT_TOOL
    {
        Some(stripped)
    } else {
        None
    }
}

// ============================================================================
// Tool Arguments
// ============================================================================

/// spawn_sub_agent 工具参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnSubAgentArgs {
    /// Sub-Agent 名称（用于标识和日志）
    pub name: String,

    /// Sub-Agent 的角色/身份描述
    /// 例如："You are a data analyst specialized in sales metrics."
    pub role: String,

    /// 要执行的任务描述
    pub task: String,

    /// 允许使用的工具列表（可选）
    /// 如果不指定，将使用系统默认配置
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,

    /// 是否等待完成（同步执行）
    /// 默认为 false（异步执行）
    #[serde(default)]
    pub wait_for_completion: bool,

    /// 超时时间（秒），可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// 最大迭代次数，可选
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,
}

/// get_sub_agent_result 工具参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSubAgentResultArgs {
    /// Sub-Agent ID
    pub subagent_id: String,

    /// 是否等待完成（如果 Sub-Agent 仍在运行）
    #[serde(default)]
    pub wait: bool,

    /// 等待超时时间（秒），仅当 wait=true 时有效
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,
}

/// cancel_sub_agent 工具参数
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelSubAgentArgs {
    /// Sub-Agent ID
    pub subagent_id: String,

    /// 取消原因（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ============================================================================
// Tool Result Types
// ============================================================================

/// spawn_sub_agent 工具返回结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnSubAgentResult {
    /// 是否成功
    pub success: bool,

    /// Sub-Agent ID（成功时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subagent_id: Option<String>,

    /// 错误信息（失败时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// 执行结果（同步模式下，wait_for_completion=true）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<SubAgentResultSummary>,
}

/// get_sub_agent_result 工具返回结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetSubAgentResultResponse {
    /// Sub-Agent ID
    pub subagent_id: String,

    /// 当前状态
    pub state: String,

    /// 执行结果（仅终态时有值）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<SubAgentResultSummary>,

    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// cancel_sub_agent 工具返回结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelSubAgentResult {
    /// 是否成功
    pub success: bool,

    /// Sub-Agent ID
    pub subagent_id: String,

    /// 新状态
    pub state: String,

    /// 错误信息（失败时）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Sub-Agent 执行结果摘要（用于工具返回）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResultSummary {
    /// 是否成功完成
    pub success: bool,

    /// 输出内容
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,

    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// 执行的迭代次数
    pub iterations: u32,

    /// 执行时长（毫秒）
    pub duration_ms: u64,
}

// ============================================================================
// Tool Schema Generation
// ============================================================================

/// 工具描述（用于 LLM）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentToolDescription {
    /// 工具名称
    pub name: String,
    /// 工具描述
    pub description: String,
}

/// 获取 spawn_sub_agent 工具描述
pub fn spawn_sub_agent_tool_description() -> SubAgentToolDescription {
    SubAgentToolDescription {
        name: subagent_tool_name(SPAWN_SUB_AGENT_TOOL),
        description: "Create and start a new Sub-Agent to execute a specific task autonomously. \
            The Sub-Agent will have its own context and can use available tools. \
            Use this when you need to delegate a complex subtask that requires independent reasoning. \
            Returns the Sub-Agent ID immediately (async mode) or waits for completion (sync mode)."
            .to_string(),
    }
}

/// 获取 get_sub_agent_result 工具描述
pub fn get_sub_agent_result_tool_description() -> SubAgentToolDescription {
    SubAgentToolDescription {
        name: subagent_tool_name(GET_SUB_AGENT_RESULT_TOOL),
        description: "Get the current status and result of a Sub-Agent. \
            Use this to check if a Sub-Agent has completed its task and retrieve its output. \
            Can optionally wait for completion if the Sub-Agent is still running."
            .to_string(),
    }
}

/// 获取 cancel_sub_agent 工具描述
pub fn cancel_sub_agent_tool_description() -> SubAgentToolDescription {
    SubAgentToolDescription {
        name: subagent_tool_name(CANCEL_SUB_AGENT_TOOL),
        description: "Cancel a running Sub-Agent. \
            Use this when you no longer need the Sub-Agent's result or want to stop a long-running task. \
            The Sub-Agent will be marked as cancelled and any ongoing work will be aborted."
            .to_string(),
    }
}

/// 获取所有 Sub-Agent 工具描述
pub fn all_subagent_tool_descriptions() -> Vec<SubAgentToolDescription> {
    vec![
        spawn_sub_agent_tool_description(),
        get_sub_agent_result_tool_description(),
        cancel_sub_agent_tool_description(),
    ]
}

/// 生成 spawn_sub_agent 工具的 JSON Schema（用于 OpenAI function calling）
pub fn spawn_sub_agent_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": subagent_tool_name(SPAWN_SUB_AGENT_TOOL),
            "description": "Create and start a new Sub-Agent to execute a specific task autonomously. The Sub-Agent will have its own context and can use available tools.",
            "parameters": {
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "A descriptive name for the Sub-Agent (e.g., 'DataAnalyst', 'CodeReviewer')"
                    },
                    "role": {
                        "type": "string",
                        "description": "The role or identity of the Sub-Agent. This becomes its system prompt."
                    },
                    "task": {
                        "type": "string",
                        "description": "The specific task for the Sub-Agent to accomplish."
                    },
                    "allowed_tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional list of tool names the Sub-Agent is allowed to use. If not specified, uses system defaults."
                    },
                    "wait_for_completion": {
                        "type": "boolean",
                        "description": "If true, wait for the Sub-Agent to complete before returning. Default is false (async).",
                        "default": false
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout in seconds. Uses system default if not specified."
                    },
                    "max_iterations": {
                        "type": "integer",
                        "description": "Optional maximum number of reasoning iterations. Uses system default if not specified."
                    }
                },
                "required": ["name", "role", "task"]
            }
        }
    })
}

/// 生成 get_sub_agent_result 工具的 JSON Schema
pub fn get_sub_agent_result_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": subagent_tool_name(GET_SUB_AGENT_RESULT_TOOL),
            "description": "Get the current status and result of a Sub-Agent.",
            "parameters": {
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "The ID of the Sub-Agent (returned by spawn_sub_agent)"
                    },
                    "wait": {
                        "type": "boolean",
                        "description": "If true, wait for the Sub-Agent to complete if still running. Default is false.",
                        "default": false
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout in seconds when waiting. Only used if wait=true."
                    }
                },
                "required": ["subagent_id"]
            }
        }
    })
}

/// 生成 cancel_sub_agent 工具的 JSON Schema
pub fn cancel_sub_agent_schema() -> Value {
    json!({
        "type": "function",
        "function": {
            "name": subagent_tool_name(CANCEL_SUB_AGENT_TOOL),
            "description": "Cancel a running Sub-Agent.",
            "parameters": {
                "type": "object",
                "properties": {
                    "subagent_id": {
                        "type": "string",
                        "description": "The ID of the Sub-Agent to cancel"
                    },
                    "reason": {
                        "type": "string",
                        "description": "Optional reason for cancellation"
                    }
                },
                "required": ["subagent_id"]
            }
        }
    })
}

/// 获取所有 Sub-Agent 工具的 JSON Schema
pub fn all_subagent_tool_schemas() -> Vec<Value> {
    vec![
        spawn_sub_agent_schema(),
        get_sub_agent_result_schema(),
        cancel_sub_agent_schema(),
    ]
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_tool_name() {
        assert_eq!(
            subagent_tool_name(SPAWN_SUB_AGENT_TOOL),
            "internal__spawn_sub_agent"
        );
        assert_eq!(
            subagent_tool_name(GET_SUB_AGENT_RESULT_TOOL),
            "internal__get_sub_agent_result"
        );
        assert_eq!(
            subagent_tool_name(CANCEL_SUB_AGENT_TOOL),
            "internal__cancel_sub_agent"
        );
    }

    #[test]
    fn test_is_subagent_tool() {
        assert!(is_subagent_tool("internal__spawn_sub_agent"));
        assert!(is_subagent_tool("internal__get_sub_agent_result"));
        assert!(is_subagent_tool("internal__cancel_sub_agent"));

        assert!(!is_subagent_tool("internal__skill_run_script"));
        assert!(!is_subagent_tool("mcp__server__tool"));
        assert!(!is_subagent_tool("spawn_sub_agent"));
    }

    #[test]
    fn test_parse_subagent_tool_name() {
        assert_eq!(
            parse_subagent_tool_name("internal__spawn_sub_agent"),
            Some(SPAWN_SUB_AGENT_TOOL)
        );
        assert_eq!(
            parse_subagent_tool_name("internal__get_sub_agent_result"),
            Some(GET_SUB_AGENT_RESULT_TOOL)
        );
        assert_eq!(
            parse_subagent_tool_name("internal__cancel_sub_agent"),
            Some(CANCEL_SUB_AGENT_TOOL)
        );

        assert_eq!(parse_subagent_tool_name("internal__skill_run_script"), None);
        assert_eq!(parse_subagent_tool_name("mcp__server__tool"), None);
    }

    #[test]
    fn test_spawn_sub_agent_args_serialization() {
        let args = SpawnSubAgentArgs {
            name: "TestAgent".to_string(),
            role: "You are a test agent".to_string(),
            task: "Do something".to_string(),
            allowed_tools: Some(vec!["tool_a".to_string()]),
            wait_for_completion: true,
            timeout_secs: Some(60),
            max_iterations: Some(10),
        };

        let json = serde_json::to_string(&args).unwrap();
        let parsed: SpawnSubAgentArgs = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "TestAgent");
        assert_eq!(parsed.role, "You are a test agent");
        assert!(parsed.wait_for_completion);
        assert_eq!(parsed.timeout_secs, Some(60));
    }

    #[test]
    fn test_spawn_sub_agent_args_minimal() {
        let json = r#"{
            "name": "Agent",
            "role": "Helper",
            "task": "Help"
        }"#;

        let args: SpawnSubAgentArgs = serde_json::from_str(json).unwrap();
        assert_eq!(args.name, "Agent");
        assert!(!args.wait_for_completion);
        assert!(args.allowed_tools.is_none());
        assert!(args.timeout_secs.is_none());
    }

    #[test]
    fn test_get_sub_agent_result_args() {
        let args = GetSubAgentResultArgs {
            subagent_id: "sa-12345678".to_string(),
            wait: true,
            timeout_secs: Some(30),
        };

        let json = serde_json::to_string(&args).unwrap();
        assert!(json.contains("sa-12345678"));
        assert!(json.contains("\"wait\":true"));
    }

    #[test]
    fn test_spawn_result_success() {
        let result = SpawnSubAgentResult {
            success: true,
            subagent_id: Some("sa-12345678".to_string()),
            error: None,
            result: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("sa-12345678"));
        assert!(!json.contains("error"));
    }

    #[test]
    fn test_spawn_result_with_sync_result() {
        let result = SpawnSubAgentResult {
            success: true,
            subagent_id: Some("sa-12345678".to_string()),
            error: None,
            result: Some(SubAgentResultSummary {
                success: true,
                output: Some("Task completed".to_string()),
                error: None,
                iterations: 3,
                duration_ms: 5000,
            }),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Task completed"));
        assert!(json.contains("\"iterations\":3"));
    }

    #[test]
    fn test_tool_descriptions() {
        let descriptions = all_subagent_tool_descriptions();
        assert_eq!(descriptions.len(), 3);

        assert!(descriptions[0].name.contains(SPAWN_SUB_AGENT_TOOL));
        assert!(descriptions[1].name.contains(GET_SUB_AGENT_RESULT_TOOL));
        assert!(descriptions[2].name.contains(CANCEL_SUB_AGENT_TOOL));
    }

    #[test]
    fn test_spawn_schema() {
        let schema = spawn_sub_agent_schema();
        assert_eq!(schema["type"], "function");
        assert!(
            schema["function"]["name"]
                .as_str()
                .unwrap()
                .contains("spawn_sub_agent")
        );

        let params = &schema["function"]["parameters"];
        assert!(params["properties"]["name"].is_object());
        assert!(params["properties"]["role"].is_object());
        assert!(params["properties"]["task"].is_object());

        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&json!("name")));
        assert!(required.contains(&json!("role")));
        assert!(required.contains(&json!("task")));
    }

    #[test]
    fn test_get_result_schema() {
        let schema = get_sub_agent_result_schema();
        assert_eq!(schema["type"], "function");

        let params = &schema["function"]["parameters"];
        assert!(params["properties"]["subagent_id"].is_object());
        assert!(params["properties"]["wait"].is_object());

        let required = params["required"].as_array().unwrap();
        assert!(required.contains(&json!("subagent_id")));
    }

    #[test]
    fn test_cancel_schema() {
        let schema = cancel_sub_agent_schema();
        assert_eq!(schema["type"], "function");

        let params = &schema["function"]["parameters"];
        assert!(params["properties"]["subagent_id"].is_object());
        assert!(params["properties"]["reason"].is_object());
    }

    #[test]
    fn test_all_schemas() {
        let schemas = all_subagent_tool_schemas();
        assert_eq!(schemas.len(), 3);

        for schema in &schemas {
            assert_eq!(schema["type"], "function");
            assert!(schema["function"]["name"].is_string());
            assert!(schema["function"]["description"].is_string());
            assert!(schema["function"]["parameters"].is_object());
        }
    }
}
