//! Public API for chat execution without Axum dependencies.
//!
//! This module provides a standalone entry point for Plan mode execution
//! that can be used by external integrations (e.g., Tauri desktop apps)
//! without depending on Axum's HTTP framework.
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use aries::chat::api::{execute_plan, ExecutePlanRequest};
//! use aries::chat::emitter::EventEmitter;
//! use tokio_util::sync::CancellationToken;
//!
//! // Create a custom event emitter
//! struct MyEmitter { /* ... */ }
//! impl EventEmitter for MyEmitter { /* ... */ }
//!
//! // Execute plan with custom emitter
//! let result = execute_plan(
//!     state,
//!     ExecutePlanRequest {
//!         message: "What's the weather?".to_string(),
//!         model: Some("gpt-4".to_string()),
//!         conversation_id: None,
//!     },
//!     Arc::new(MyEmitter::new()),
//!     CancellationToken::new(),
//! ).await?;
//!
//! println!("Response: {}", result.content);
//! ```

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use super::emitter::EventEmitter;
use super::events::{ExecutionPhase, ExecutionSummary};
use super::planner::TaskPlanner;
use super::trace::{PlanTrace, SubtaskTrace, TokenUsage, TraceStatus};
use crate::chat::planner::ToolDescription;
use crate::error::{ServerError, ServerResult};
use crate::server::{RoutingPolicy, ServerKind};
use crate::skills::SkillRegistry;
use crate::{dual_error, dual_info, dual_warn, AppState};

// Re-export for convenience
pub use super::emitter::{NoopEventEmitter, SseEventEmitter};
pub use super::events::{
    ExecutionPhase as Phase, StreamEvent, StreamEventType, ThoughtStatus as Thought,
};

// ============================================================================
// Request/Response Types
// ============================================================================

/// Request for executing a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutePlanRequest {
    /// The user message to process.
    pub message: String,

    /// The model to use for chat completions.
    /// If not specified, uses the default model from configuration.
    #[serde(default)]
    pub model: Option<String>,

    /// Optional conversation ID for memory persistence.
    #[serde(default)]
    pub conversation_id: Option<String>,

    /// Optional system message to set context.
    #[serde(default)]
    pub system_message: Option<String>,
}

/// Response from plan execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutePlanResponse {
    /// The final response content.
    pub content: String,

    /// Token usage statistics.
    pub usage: TokenUsage,

    /// Execution summary.
    pub summary: Option<ExecutionSummary>,
}

// ============================================================================
// Public API
// ============================================================================

/// Executes a plan without Axum dependencies.
///
/// This is the main entry point for Plan mode execution that can be used by
/// external integrations like Tauri desktop applications.
///
/// # Arguments
///
/// * `state` - The application state containing configuration and services
/// * `request` - The execution request with user message and options
/// * `emitter` - Custom event emitter for receiving execution events
/// * `cancel_token` - Token for cancelling the execution
///
/// # Returns
///
/// Returns the execution response containing the final content and usage stats.
///
/// # Errors
///
/// Returns an error if:
/// - No chat server is available
/// - Task planning fails
/// - Execution times out
/// - The request is cancelled
pub async fn execute_plan(
    state: Arc<AppState>,
    request: ExecutePlanRequest,
    emitter: Arc<dyn EventEmitter>,
    cancel_token: CancellationToken,
) -> ServerResult<ExecutePlanResponse> {
    let request_id = format!("plan-{}", uuid::Uuid::new_v4());

    dual_info!(
        "🚀 Starting plan execution via API - request_id: {}",
        request_id
    );

    // Get target server
    let chat_server = get_chat_server(&state, &request_id).await?;

    dual_info!(
        "📡 Chat server: url={}, model={:?} - request_id: {}",
        chat_server.url,
        request.model,
        request_id
    );

    // Get plan mode configuration
    let (
        max_plan_subtasks,
        plan_timeout_secs,
        _subtask_max_retries,
        subtask_react_max_iterations,
        subtask_react_timeout_secs,
        max_tools_per_iteration,
        tool_call_max_retries,
        tool_call_retry_delay_ms,
        _reflection_config,
    ) = {
        let config = state.config.read().await;
        (
            config.server.max_plan_subtasks,
            config.server.plan_timeout_secs,
            config.server.subtask_max_retries,
            config.server.subtask_react_max_iterations,
            config.server.subtask_react_timeout_secs,
            config.server.max_tools_per_iteration,
            config.server.tool_call_max_retries,
            config.server.tool_call_retry_delay_ms,
            config.reflection.clone().unwrap_or_default(),
        )
    };

    // Initialize time budget
    let time_budget = super::shared::TimeBudget::new(plan_timeout_secs);

    // Emit planning status
    emitter
        .emit_status(
            ExecutionPhase::Planning,
            "Analyzing user request and generating task plan...",
            None,
            None,
            None,
        )
        .await;

    // Get available tools
    let available_tools = get_available_tools().await;

    // Get skills summaries
    let skills_summaries = match SkillRegistry::global() {
        Ok(registry) => {
            let summaries = registry.get_summaries().await;
            if !summaries.is_empty() {
                dual_info!(
                    "📚 Loaded {} skills for planning - request_id: {}",
                    summaries.len(),
                    request_id
                );
            }
            summaries
        }
        Err(_) => vec![],
    };

    // Get model name
    let model_name = request
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());

    // Create task planner
    let planner = TaskPlanner::with_chat_llm(
        format!(
            "{}/chat/completions",
            chat_server.url.trim_end_matches('/')
        ),
        chat_server.api_key.clone(),
        model_name.clone(),
        max_plan_subtasks,
    )
    .with_tools(available_tools.clone())
    .with_skills(skills_summaries.clone());

    // Generate task plan
    let plan = match planner.plan(&request.message).await {
        Ok(plan) => plan,
        Err(e) => {
            dual_error!(
                "Failed to generate task plan: {} - request_id: {}",
                e,
                request_id
            );
            return Err(e);
        }
    };

    dual_info!(
        "📋 Task plan generated: {} subtasks - request_id: {}",
        plan.len(),
        request_id
    );

    // Initialize trace
    let execution_order: Vec<usize> = plan.subtasks.iter().map(|s| s.id).collect();
    let mut trace = PlanTrace::new(
        request_id.clone(),
        request.message.clone(),
        execution_order,
    );
    trace.start();

    // Execute subtasks
    let mut subtask_results: Vec<(usize, String)> = Vec::new();

    for (index, subtask) in plan.subtasks.iter().enumerate() {
        // Check cancellation
        if cancel_token.is_cancelled() {
            return Err(ServerError::Operation(
                "Request was cancelled by client".to_string(),
            ));
        }

        // Check time budget
        if time_budget.is_exhausted() {
            dual_warn!(
                "Plan execution timeout - request_id: {}",
                request_id
            );
            break;
        }

        // Emit executing status
        emitter
            .emit_status(
                ExecutionPhase::Executing,
                &format!("Executing subtask: {}", subtask.description),
                Some(subtask.id),
                Some(index + 1),
                Some(plan.subtasks.len()),
            )
            .await;

        // Create subtask trace
        let mut subtask_trace = SubtaskTrace::new(subtask.id, subtask.description.clone());
        subtask_trace.start();

        // Execute subtask using React loop
        let result = super::plan::execute_subtask_with_react_api(
            &state,
            &chat_server,
            subtask,
            &subtask_results,
            &available_tools,
            Some(&skills_summaries),
            request.conversation_id.as_deref(),
            std::time::Duration::from_secs(subtask_react_timeout_secs),
            subtask_react_max_iterations,
            max_tools_per_iteration,
            tool_call_max_retries,
            tool_call_retry_delay_ms,
            &cancel_token,
            &request_id,
            &mut subtask_trace,
            &model_name,
            emitter.as_ref(),
        )
        .await;

        match result {
            Ok(content) => {
                subtask_trace.complete(content.clone());
                subtask_results.push((subtask.id, content));
                trace.add_subtask_trace(subtask_trace);
            }
            Err(e) => {
                dual_error!(
                    "Subtask {} failed: {} - request_id: {}",
                    subtask.id,
                    e,
                    request_id
                );
                subtask_trace.fail(e.to_string());
                trace.add_subtask_trace(subtask_trace);

                // Continue with other subtasks unless it's a fatal error
                if matches!(e, ServerError::Operation(_)) {
                    continue;
                }
                return Err(e);
            }
        }
    }

    // Emit completing status
    emitter
        .emit_status(
            ExecutionPhase::Completing,
            "Generating final response...",
            None,
            None,
            None,
        )
        .await;

    // Generate final response
    let final_content = generate_final_response(
        &state,
        &chat_server,
        &request.message,
        &subtask_results,
        &model_name,
        &request_id,
    )
    .await?;

    // Emit the final content as a text event so the UI can display it
    emitter.emit_text(&final_content).await;

    // Store to memory if conversation ID provided
    if let (Some(memory), Some(conv_id)) = (&state.memory, &request.conversation_id) {
        // Store user message
        if let Err(e) = memory.add_user_message(conv_id, request.message.clone()).await {
            dual_error!(
                "Failed to add user message to memory: {} - request_id: {}",
                e,
                request_id
            );
        }

        // Store assistant message
        if let Err(e) = memory
            .add_assistant_message(conv_id, &final_content, vec![])
            .await
        {
            dual_error!(
                "Failed to add assistant message to memory: {} - request_id: {}",
                e,
                request_id
            );
        }
    }

    // Finalize trace
    trace.finalize(TraceStatus::Success);

    // Calculate execution statistics
    let completed_count = trace
        .subtask_traces
        .iter()
        .filter(|t| matches!(t.status, crate::chat::planner::SubTaskStatus::Completed))
        .count();
    let failed_count = trace
        .subtask_traces
        .iter()
        .filter(|t| matches!(t.status, crate::chat::planner::SubTaskStatus::Failed(_)))
        .count();
    let tool_call_count: usize = trace
        .subtask_traces
        .iter()
        .flat_map(|t| t.react_iterations.iter())
        .map(|it| it.tool_calls.len())
        .sum();

    let execution_summary = ExecutionSummary {
        subtask_count: trace.subtask_traces.len(),
        completed_count,
        failed_count,
        tool_call_count,
        duration_ms: trace.total_duration.as_millis() as u64,
    };

    // Emit finish event
    emitter
        .emit_finish(
            &trace.total_tokens,
            "stop",
            None,
            Some(execution_summary.clone()),
        )
        .await;

    dual_info!("✅ Plan execution completed - request_id: {}", request_id);

    Ok(ExecutePlanResponse {
        content: final_content,
        usage: trace.total_tokens,
        summary: Some(execution_summary),
    })
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Gets the chat server for making LLM requests.
async fn get_chat_server(
    state: &Arc<AppState>,
    request_id: &str,
) -> ServerResult<crate::server::TargetServerInfo> {
    let servers = state.server_group.read().await;
    let chat_servers = match servers.get(&ServerKind::chat) {
        Some(servers) => servers,
        None => {
            let err_msg = "No chat server available";
            dual_error!("{} - request_id: {}", err_msg, request_id);
            return Err(ServerError::Operation(err_msg.to_string()));
        }
    };

    match chat_servers.next().await {
        Ok(target_server_info) => Ok(target_server_info),
        Err(e) => {
            let err_msg = format!("Failed to get the chat server: {e}");
            dual_error!("{} - request_id: {}", err_msg, request_id);
            Err(ServerError::Operation(err_msg))
        }
    }
}

/// Gets available tools from MCP services.
async fn get_available_tools() -> Vec<ToolDescription> {
    use crate::mcp::{MCP_SERVICES, format_mcp_tool_name};
    use crate::skills::SkillRegistry;

    let mut tools = Vec::new();

    // Add MCP tools
    if let Some(services) = MCP_SERVICES.get() {
        let service_map = services.read().await;
        for (server_name, service) in service_map.iter() {
            let service_read = service.read().await;
            for tool_name in &service_read.tools {
                tools.push(ToolDescription {
                    name: format_mcp_tool_name(server_name, tool_name),
                    description: format!("Tool {} from {}", tool_name, server_name),
                });
            }
        }
    }

    // Add internal tools
    if SkillRegistry::global().is_ok() {
        tools.push(ToolDescription {
            name: "internal__skill_run_script".to_string(),
            description: "Execute a script from an active skill".to_string(),
        });
        tools.push(ToolDescription {
            name: "internal__skill_load_asset".to_string(),
            description: "Load an asset file from the active skill".to_string(),
        });
    }

    tools
}

/// Generates the final response by summarizing subtask results.
async fn generate_final_response(
    _state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    user_message: &str,
    subtask_results: &[(usize, String)],
    model: &str,
    _request_id: &str,
) -> ServerResult<String> {
    use super::shared::{extract_chat_content, send_llm_request};

    // If no subtasks completed, return a default message
    if subtask_results.is_empty() {
        return Ok("I apologize, but I was unable to complete the requested task.".to_string());
    }

    // If only one subtask, return its result directly
    if subtask_results.len() == 1 {
        return Ok(subtask_results[0].1.clone());
    }

    // Build summary prompt
    let results_summary: String = subtask_results
        .iter()
        .map(|(id, result)| format!("Subtask {}: {}", id, result))
        .collect::<Vec<_>>()
        .join("\n\n");

    let summary_prompt = format!(
        "Based on the following subtask results, provide a comprehensive response to the user's original question.\n\n\
        User's question: {}\n\n\
        Subtask results:\n{}\n\n\
        Provide a clear, helpful response that synthesizes these results.",
        user_message, results_summary
    );

    // Call LLM for final summary using curl
    let url = format!(
        "{}/chat/completions",
        chat_server.url.trim_end_matches('/')
    );

    let request_json = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant that synthesizes information from multiple sources into a coherent response."
            },
            {
                "role": "user",
                "content": summary_prompt
            }
        ],
        "stream": false
    });

    let response = send_llm_request(&url, chat_server.api_key.as_deref(), &request_json)?;
    let content = extract_chat_content(&response).unwrap_or_else(|_| "Unable to generate summary.".to_string());

    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execute_plan_request_serialization() {
        let request = ExecutePlanRequest {
            message: "What's the weather?".to_string(),
            model: Some("gpt-4".to_string()),
            conversation_id: None,
            system_message: None,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("What's the weather?"));
        assert!(json.contains("gpt-4"));
    }

    #[test]
    fn test_execute_plan_response_serialization() {
        let response = ExecutePlanResponse {
            content: "The weather is sunny.".to_string(),
            usage: TokenUsage::new(100, 50),
            summary: Some(ExecutionSummary {
                subtask_count: 2,
                completed_count: 2,
                failed_count: 0,
                tool_call_count: 1,
                duration_ms: 1500,
            }),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("The weather is sunny."));
        assert!(json.contains("\"subtask_count\":2"));
    }
}
