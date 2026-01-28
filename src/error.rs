use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use thiserror::Error;

pub type ServerResult<T> = std::result::Result<T, ServerError>;

#[derive(Error, Debug, Clone)]
pub enum ServerError {
    #[error("{0}")]
    Operation(String),
    #[error(
        "Not found available server. Please register a(n) {0} server via the `/admin/servers/register` endpoint."
    )]
    NotFoundServer(String),
    #[error("Invalid server kind: {0}")]
    InvalidServerKind(String),
    #[error("Failed to load config: {0}")]
    FailedToLoadConfig(String),
    #[error("Mcp server returned empty content")]
    McpEmptyContent,
    #[error("Mcp operation failed: {0}")]
    McpOperation(String),
    #[error("React loop exceeded maximum iterations ({0})")]
    MaxIterationsExceeded(u32),
    #[error("Tool call '{tool_name}' failed after {attempts} retries: {message}")]
    ToolCallRetryExhausted {
        tool_name: String,
        attempts: u32,
        message: String,
    },
    #[allow(dead_code)]
    #[error("Invalid XML tag format: {0}")]
    InvalidXmlTag(String),
    // Plan mode errors
    #[error("Failed to parse task plan: {0}")]
    PlanParseError(String),
    #[error("Cyclic dependency detected in task plan")]
    CyclicDependency,
    #[error("Invalid subtask reference: {0}")]
    InvalidReference(String),
    #[error("Task plan is empty")]
    EmptyPlan,
    #[error("Subtask '{subtask_id}' failed after {attempts} retries: {message}")]
    SubtaskRetryExhausted {
        subtask_id: usize,
        attempts: u32,
        message: String,
    },
    #[error("Subtask '{subtask_id}' timeout after {timeout_secs} seconds")]
    SubtaskTimeout {
        subtask_id: usize,
        timeout_secs: u64,
    },
    #[error("Plan time budget exhausted after {elapsed_secs} seconds")]
    TimeBudgetExhausted { elapsed_secs: u64 },

    // Sub-Agent errors
    #[error("Sub-Agent feature is disabled")]
    SubAgentDisabled,
    #[error("Sub-Agent not found: {0}")]
    SubAgentNotFound(String),
    #[error("Sub-Agent concurrent limit exceeded: max {max} allowed")]
    SubAgentLimitExceeded { max: usize },
    #[error(
        "Sub-Agent max nesting depth exceeded: max depth {max_depth}, current depth {current_depth}"
    )]
    SubAgentMaxDepthExceeded { max_depth: u32, current_depth: u32 },
    #[error("Sub-Agent spawn failed: {0}")]
    SubAgentSpawnFailed(String),
    #[error("Sub-Agent execution failed: {id} - {message}")]
    SubAgentExecutionFailed { id: String, message: String },
    #[error("Sub-Agent timeout: {id} after {timeout_secs} seconds")]
    SubAgentTimeout { id: String, timeout_secs: u64 },
    #[error("Sub-Agent already in terminal state: {id} is {state}")]
    SubAgentAlreadyTerminal { id: String, state: String },
    #[error("Sub-Agent global token limit exceeded: used {used} of {max} tokens")]
    SubAgentTokenLimitExceeded { used: u64, max: u64 },

    // User interruption errors
    #[error("User interrupted: {0}")]
    UserInterrupted(String),
}
impl IntoResponse for ServerError {
    fn into_response(self) -> Response {
        let (status, message, error_type, param, code) = match &self {
            ServerError::Operation(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                e.clone(),
                "internal_error".into(),
                None,
                Some("operation_failed".into()),
            ),
            ServerError::NotFoundServer(kind) => (
                StatusCode::NOT_FOUND,
                format!(
                    "Not found available server. Please register a(n) {kind} server via the `/admin/servers/register` endpoint."
                ),
                "not_found".into(),
                Some("server_kind".into()),
                Some("not_found_server".into()),
            ),
            ServerError::InvalidServerKind(kind) => (
                StatusCode::BAD_REQUEST,
                format!("Invalid server kind: {kind}"),
                "invalid_request_error".into(),
                Some("server_kind".into()),
                Some("invalid_server_kind".into()),
            ),
            ServerError::FailedToLoadConfig(e) => (
                StatusCode::BAD_REQUEST,
                format!("Failed to load config: {e}"),
                "invalid_request_error".into(),
                Some("config".into()),
                Some("failed_to_load_config".into()),
            ),
            ServerError::McpEmptyContent => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Mcp server returned empty content".into(),
                "internal_error".into(),
                None,
                Some("mcp_empty".into()),
            ),
            ServerError::McpOperation(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Mcp operation failed: {e}"),
                "internal_error".into(),
                None,
                Some("mcp_operation_failed".into()),
            ),
            ServerError::MaxIterationsExceeded(max) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("React loop exceeded maximum iterations ({max})"),
                "internal_error".into(),
                Some("max_react_iterations".into()),
                Some("max_iterations_exceeded".into()),
            ),
            ServerError::ToolCallRetryExhausted {
                tool_name,
                attempts,
                message,
            } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Tool call '{tool_name}' failed after {attempts} retries: {message}"),
                "internal_error".into(),
                Some("tool_call".into()),
                Some("tool_call_retry_exhausted".into()),
            ),
            ServerError::InvalidXmlTag(tag) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Invalid XML tag format: {tag}"),
                "internal_error".into(),
                Some("xml_tag".into()),
                Some("invalid_xml_tag".into()),
            ),
            ServerError::PlanParseError(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to parse task plan: {e}"),
                "internal_error".into(),
                Some("task_plan".into()),
                Some("plan_parse_error".into()),
            ),
            ServerError::CyclicDependency => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cyclic dependency detected in task plan".into(),
                "internal_error".into(),
                Some("task_plan".into()),
                Some("cyclic_dependency".into()),
            ),
            ServerError::InvalidReference(ref_info) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Invalid subtask reference: {ref_info}"),
                "internal_error".into(),
                Some("task_plan".into()),
                Some("invalid_reference".into()),
            ),
            ServerError::EmptyPlan => (
                StatusCode::BAD_REQUEST,
                "Task plan is empty".into(),
                "invalid_request_error".into(),
                Some("task_plan".into()),
                Some("empty_plan".into()),
            ),
            ServerError::SubtaskRetryExhausted {
                subtask_id,
                attempts,
                message,
            } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Subtask '{subtask_id}' failed after {attempts} retries: {message}"),
                "internal_error".into(),
                Some("subtask".into()),
                Some("subtask_retry_exhausted".into()),
            ),
            ServerError::SubtaskTimeout {
                subtask_id,
                timeout_secs,
            } => (
                StatusCode::GATEWAY_TIMEOUT,
                format!("Subtask '{subtask_id}' timeout after {timeout_secs} seconds"),
                "timeout_error".into(),
                Some("subtask_react_timeout_secs".into()),
                Some("subtask_timeout".into()),
            ),
            ServerError::TimeBudgetExhausted { elapsed_secs } => (
                StatusCode::GATEWAY_TIMEOUT,
                format!("Plan time budget exhausted after {elapsed_secs} seconds"),
                "timeout_error".into(),
                Some("plan_timeout_secs".into()),
                Some("time_budget_exhausted".into()),
            ),
            // Sub-Agent errors
            ServerError::SubAgentDisabled => (
                StatusCode::SERVICE_UNAVAILABLE,
                "Sub-Agent feature is disabled".into(),
                "service_unavailable".into(),
                Some("subagent".into()),
                Some("subagent_disabled".into()),
            ),
            ServerError::SubAgentNotFound(id) => (
                StatusCode::NOT_FOUND,
                format!("Sub-Agent not found: {id}"),
                "not_found".into(),
                Some("subagent_id".into()),
                Some("subagent_not_found".into()),
            ),
            ServerError::SubAgentLimitExceeded { max } => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Sub-Agent concurrent limit exceeded: max {max} allowed"),
                "rate_limit_error".into(),
                Some("max_concurrent".into()),
                Some("subagent_limit_exceeded".into()),
            ),
            ServerError::SubAgentMaxDepthExceeded {
                max_depth,
                current_depth,
            } => (
                StatusCode::BAD_REQUEST,
                format!(
                    "Sub-Agent max nesting depth exceeded: max depth {max_depth}, current depth {current_depth}"
                ),
                "invalid_request_error".into(),
                Some("max_nesting_depth".into()),
                Some("subagent_max_depth_exceeded".into()),
            ),
            ServerError::SubAgentSpawnFailed(reason) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sub-Agent spawn failed: {reason}"),
                "internal_error".into(),
                Some("subagent".into()),
                Some("subagent_spawn_failed".into()),
            ),
            ServerError::SubAgentExecutionFailed { id, message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Sub-Agent execution failed: {id} - {message}"),
                "internal_error".into(),
                Some("subagent".into()),
                Some("subagent_execution_failed".into()),
            ),
            ServerError::SubAgentTimeout { id, timeout_secs } => (
                StatusCode::GATEWAY_TIMEOUT,
                format!("Sub-Agent timeout: {id} after {timeout_secs} seconds"),
                "timeout_error".into(),
                Some("subagent_timeout_secs".into()),
                Some("subagent_timeout".into()),
            ),
            ServerError::SubAgentAlreadyTerminal { id, state } => (
                StatusCode::CONFLICT,
                format!("Sub-Agent already in terminal state: {id} is {state}"),
                "conflict_error".into(),
                Some("subagent_state".into()),
                Some("subagent_already_terminal".into()),
            ),
            ServerError::SubAgentTokenLimitExceeded { used, max } => (
                StatusCode::TOO_MANY_REQUESTS,
                format!("Sub-Agent global token limit exceeded: used {used} of {max} tokens"),
                "rate_limit_error".into(),
                Some("token_limit".into()),
                Some("subagent_token_limit_exceeded".into()),
            ),
            // User interruption
            ServerError::UserInterrupted(reason) => (
                StatusCode::OK, // Use 200 OK since this is expected user behavior
                format!("User interrupted: {reason}"),
                "user_interrupted".into(),
                None,
                Some("user_interrupted".into()),
            ),
        };

        let body = OpenAIErrorResponse {
            error: OpenAIError {
                message,
                error_type,
                param,
                code,
            },
        };

        (status, Json(body)).into_response()
    }
}

#[derive(Serialize)]
struct OpenAIErrorResponse {
    error: OpenAIError,
}

#[derive(Serialize)]
struct OpenAIError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    param: Option<String>,
    code: Option<String>,
}
