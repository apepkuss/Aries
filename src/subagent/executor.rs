//! Sub-Agent 执行器
//!
//! 此模块实现 SubAgentExecutor，负责执行 Sub-Agent 的任务。
//! 复用现有的 React 循环逻辑，但使用独立的上下文。

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use endpoints::chat::ChatCompletionObject;
use http::HeaderMap;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use tokio::select;
use tokio_util::sync::CancellationToken;

use super::{
    context::SubAgentContext,
    manager::SubAgentManager,
    tools::{SubAgentResultSummary, is_subagent_tool},
    types::{SubAgentId, SubAgentMetrics, SubAgentResult},
};
use crate::{
    app::AppState,
    chat::{emitter::EventEmitter, events::ThoughtStatus, planner::ToolDescription},
    error::{ServerError, ServerResult},
    mcp::MCP_SERVICES,
    server::TargetServerInfo,
};

// ============================================================================
// SubAgentExecutor
// ============================================================================

/// Sub-Agent 执行器
///
/// 负责执行 Sub-Agent 的任务，使用 React 循环（Reason + Act）模式。
/// 每个执行器实例处理一个 Sub-Agent 的执行。
pub struct SubAgentExecutor {
    /// 应用状态
    #[allow(dead_code)]
    state: Arc<AppState>,
    /// 聊天服务器信息
    chat_server: TargetServerInfo,
    /// HTTP 头信息
    headers: HeaderMap,
    /// Sub-Agent Manager 引用
    manager: Arc<SubAgentManager>,
    /// 可用工具列表
    available_tools: Vec<ToolDescription>,
    /// 模型名称
    model: String,
}

impl SubAgentExecutor {
    /// 创建新的执行器
    pub fn new(
        state: Arc<AppState>,
        chat_server: TargetServerInfo,
        headers: HeaderMap,
        manager: Arc<SubAgentManager>,
        available_tools: Vec<ToolDescription>,
        model: String,
    ) -> Self {
        Self {
            state,
            chat_server,
            headers,
            manager,
            available_tools,
            model,
        }
    }

    /// 执行 Sub-Agent 任务
    ///
    /// # Arguments
    ///
    /// * `id` - Sub-Agent ID
    /// * `context` - 执行上下文
    /// * `timeout` - 超时时间
    /// * `max_iterations` - 最大迭代次数
    /// * `cancel_token` - 取消令牌
    /// * `emitter` - 事件发射器
    ///
    /// # Returns
    ///
    /// 执行结果
    pub async fn execute(
        &self,
        id: &SubAgentId,
        mut context: SubAgentContext,
        timeout: Duration,
        max_iterations: u32,
        cancel_token: &CancellationToken,
        emitter: &dyn EventEmitter,
    ) -> ServerResult<SubAgentResult> {
        let start_time = Instant::now();

        // 标记为开始运行
        self.manager.mark_started(id).await?;

        // 过滤工具列表
        let filtered_tools = self.filter_tools_for_context(&context);

        // 执行 React 循环
        let result = self
            .execute_react_loop(
                id,
                &mut context,
                &filtered_tools,
                timeout,
                max_iterations,
                cancel_token,
                emitter,
                start_time,
            )
            .await;

        // 更新指标
        let duration = start_time.elapsed();
        self.manager
            .update_metrics(id, |metrics| {
                metrics.total_iterations = context.current_iteration();
                metrics.set_duration(duration);
            })
            .await
            .ok(); // 忽略指标更新错误

        // 根据结果更新状态
        match result {
            Ok(output) => {
                let mut metrics = SubAgentMetrics {
                    total_iterations: context.current_iteration(),
                    ..Default::default()
                };
                metrics.set_duration(duration);
                let sub_result = SubAgentResult::success(output, metrics);
                self.manager.mark_completed(id, sub_result.clone()).await?;
                Ok(sub_result)
            }
            Err(e) => {
                let error_msg = e.to_string();
                self.manager.mark_failed(id, error_msg.clone()).await?;
                Err(e)
            }
        }
    }

    /// 执行 React 循环
    #[allow(clippy::too_many_arguments)]
    async fn execute_react_loop(
        &self,
        id: &SubAgentId,
        context: &mut SubAgentContext,
        available_tools: &[ToolDescription],
        timeout: Duration,
        max_iterations: u32,
        cancel_token: &CancellationToken,
        emitter: &dyn EventEmitter,
        start_time: Instant,
    ) -> ServerResult<String> {
        loop {
            // 检查迭代限制
            context.increment_iteration();
            let iteration = context.current_iteration();

            if iteration > max_iterations {
                return Err(ServerError::MaxIterationsExceeded(max_iterations));
            }

            // 检查超时
            if start_time.elapsed() > timeout {
                return Err(ServerError::SubAgentTimeout {
                    id: id.to_string(),
                    timeout_secs: timeout.as_secs(),
                });
            }

            // 检查取消
            if cancel_token.is_cancelled() {
                return Err(ServerError::Operation(
                    "Sub-Agent execution was cancelled".to_string(),
                ));
            }

            // 构建并发送 LLM 请求
            let chat_completion = self
                .send_llm_request(context, available_tools, cancel_token)
                .await?;

            // 解析响应
            let message = &chat_completion.choices[0].message;
            let tool_calls = &message.tool_calls;
            let content = message.content.as_ref();

            // 更新 token 使用量
            self.manager
                .update_metrics(id, |metrics| {
                    metrics.prompt_tokens += chat_completion.usage.prompt_tokens;
                    metrics.completion_tokens += chat_completion.usage.completion_tokens;
                })
                .await
                .ok();

            // 检查是否有工具调用
            if !tool_calls.is_empty() {
                // 处理工具调用
                for tool_call in tool_calls {
                    // 提取思考内容
                    if let Some(content) = content
                        && let Some(thought) = extract_thought(content)
                    {
                        emitter
                            .emit_thought(&thought, ThoughtStatus::Done, None, Some(iteration))
                            .await;
                    }

                    // 发射工具调用事件
                    let tool_args: serde_json::Value =
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::json!({}));

                    emitter
                        .emit_tool_call(
                            &tool_call.id,
                            &tool_call.function.name,
                            &tool_args,
                            None,
                            None,
                        )
                        .await;

                    // 执行工具调用
                    let tool_start = Instant::now();
                    let tool_result = self
                        .execute_tool_call(&tool_call.function.name, &tool_args)
                        .await;

                    let tool_duration = tool_start.elapsed();

                    // 发射工具结果事件
                    match &tool_result {
                        Ok(result) => {
                            emitter
                                .emit_tool_result(
                                    &tool_call.id,
                                    result,
                                    false,
                                    Some(tool_duration),
                                    None,
                                )
                                .await;
                        }
                        Err(e) => {
                            emitter
                                .emit_tool_result(
                                    &tool_call.id,
                                    &e.to_string(),
                                    true,
                                    Some(tool_duration),
                                    None,
                                )
                                .await;
                        }
                    }

                    let tool_result = tool_result?;

                    // 添加助手消息和工具结果到上下文
                    context.add_assistant_message(
                        message.content.clone(),
                        Some(vec![tool_call.clone()]),
                    );

                    let observation = format!("<observation>{}</observation>", tool_result);
                    context.add_tool_result(&observation, &tool_call.id);
                }
            } else {
                // 没有工具调用，检查是否有最终答案
                if let Some(content) = content {
                    // 检查是否包含最终答案
                    if let Some(answer) = extract_final_answer(content) {
                        return Ok(answer);
                    }

                    // 如果内容不为空且没有工具调用，视为最终响应
                    if !content.trim().is_empty() {
                        return Ok(content.to_string());
                    }
                }

                // 空响应，可能是模型问题
                return Err(ServerError::Operation(
                    "Sub-Agent returned empty response without tool calls".to_string(),
                ));
            }
        }
    }

    /// 发送 LLM 请求
    async fn send_llm_request(
        &self,
        context: &SubAgentContext,
        available_tools: &[ToolDescription],
        cancel_token: &CancellationToken,
    ) -> ServerResult<ChatCompletionObject> {
        let url = format!(
            "{}/chat/completions",
            self.chat_server.url.trim_end_matches('/')
        );

        let mut client = reqwest::Client::new().post(&url);
        client = client.header(CONTENT_TYPE, "application/json");

        // 设置认证
        if let Some(api_key) = &self.chat_server.api_key {
            if !api_key.is_empty() {
                let auth = if api_key.starts_with("Bearer ") {
                    api_key.clone()
                } else {
                    format!("Bearer {api_key}")
                };
                client = client.header(AUTHORIZATION, auth);
            }
        } else if let Some(auth) = self.headers.get("authorization")
            && let Ok(auth_str) = auth.to_str()
        {
            client = client.header(AUTHORIZATION, auth_str);
        }

        // 构建工具 JSON
        let tools_json = build_tools_json(available_tools);

        // 构建请求
        let request_json = serde_json::json!({
            "model": self.model,
            "messages": context.messages(),
            "tools": tools_json,
            "stream": false
        });

        // 发送请求（支持取消）
        let response = select! {
            response = client.json(&request_json).send() => {
                response.map_err(|e| ServerError::Operation(format!("Failed to send request: {e}")))
            }
            _ = cancel_token.cancelled() => {
                Err(ServerError::Operation("Request was cancelled".to_string()))
            }
        }?;

        // 解析响应
        response
            .json()
            .await
            .map_err(|e| ServerError::Operation(format!("Failed to parse response: {e}")))
    }

    /// 执行工具调用
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> ServerResult<String> {
        // 检查是否是 Sub-Agent 工具（不允许递归调用，除非配置允许）
        if is_subagent_tool(tool_name) {
            return Err(ServerError::Operation(
                "Sub-Agent cannot spawn another Sub-Agent by default".to_string(),
            ));
        }

        // 解析 MCP 工具名称
        let (server_name, mcp_tool_name) = parse_mcp_tool_name(tool_name).ok_or_else(|| {
            ServerError::Operation(format!("Invalid tool name format: {tool_name}"))
        })?;

        // 获取 MCP 服务
        let services = MCP_SERVICES
            .get()
            .ok_or_else(|| ServerError::Operation("MCP services not initialized".to_string()))?;

        let service_map = services.read().await;
        let service = service_map.get(server_name).ok_or_else(|| {
            ServerError::McpOperation(format!("MCP server '{}' not found", server_name))
        })?;

        // 调用工具
        let request_param = rmcp::model::CallToolRequestParam {
            name: mcp_tool_name.to_string().into(),
            arguments: serde_json::from_value(args.clone()).ok(),
        };

        let result = service
            .read()
            .await
            .raw
            .call_tool(request_param)
            .await
            .map_err(|e| ServerError::McpOperation(format!("Tool call failed: {e}")))?;

        if result.is_error == Some(true) {
            return Err(ServerError::McpOperation("Tool returned error".to_string()));
        }

        // 提取结果文本
        if !result.content.is_empty()
            && let rmcp::model::RawContent::Text(text) = &result.content[0].raw
        {
            return Ok(text.text.clone());
        }

        Err(ServerError::McpEmptyContent)
    }

    /// 根据上下文过滤工具列表
    fn filter_tools_for_context(&self, context: &SubAgentContext) -> Vec<ToolDescription> {
        self.available_tools
            .iter()
            .filter(|t| context.is_tool_allowed(&t.name))
            .cloned()
            .collect()
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 构建工具 JSON
fn build_tools_json(tools: &[ToolDescription]) -> serde_json::Value {
    let tools_json: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "query": {
                                "type": "string",
                                "description": "The query or input for the tool"
                            }
                        },
                        "required": ["query"]
                    }
                }
            })
        })
        .collect();

    serde_json::Value::Array(tools_json)
}

/// 解析 MCP 工具名称
fn parse_mcp_tool_name(full_name: &str) -> Option<(&str, &str)> {
    if !full_name.starts_with("mcp__") {
        return None;
    }

    let rest = full_name.strip_prefix("mcp__")?;
    let (server_name, tool_name) = rest.split_once("__")?;

    Some((server_name, tool_name))
}

/// 提取思考内容
fn extract_thought(content: &str) -> Option<String> {
    // 尝试提取 <thought>...</thought> 标签中的内容
    let start_tag = "<thought>";
    let end_tag = "</thought>";

    if let Some(start) = content.find(start_tag)
        && let Some(end) = content[start..].find(end_tag)
    {
        let thought_start = start + start_tag.len();
        let thought_end = start + end;
        return Some(content[thought_start..thought_end].trim().to_string());
    }

    None
}

/// 提取最终答案
fn extract_final_answer(content: &str) -> Option<String> {
    // 尝试提取 <final_answer>...</final_answer> 标签中的内容
    let start_tag = "<final_answer>";
    let end_tag = "</final_answer>";

    if let Some(start) = content.find(start_tag)
        && let Some(end) = content[start..].find(end_tag)
    {
        let answer_start = start + start_tag.len();
        let answer_end = start + end;
        return Some(content[answer_start..answer_end].trim().to_string());
    }

    None
}

// ============================================================================
// Execution Functions (for tool handlers)
// ============================================================================

/// 执行 spawn_sub_agent 工具
///
/// 创建并启动一个新的 Sub-Agent
#[allow(clippy::too_many_arguments)]
pub async fn execute_spawn_sub_agent(
    state: Arc<AppState>,
    chat_server: TargetServerInfo,
    headers: HeaderMap,
    manager: Arc<SubAgentManager>,
    available_tools: Vec<ToolDescription>,
    model: String,
    name: String,
    role: String,
    task: String,
    allowed_tools: Option<Vec<String>>,
    wait_for_completion: bool,
    timeout_secs: Option<u64>,
    max_iterations: Option<u32>,
    parent_id: Option<SubAgentId>,
    emitter: &dyn EventEmitter,
) -> ServerResult<serde_json::Value> {
    use super::config::SubAgentSpawnConfig;

    // 构建 spawn 配置
    let mut spawn_config = SubAgentSpawnConfig::new();

    if let Some(tools) = allowed_tools {
        spawn_config = spawn_config.with_allowed_tools(tools);
    }
    if let Some(timeout) = timeout_secs {
        spawn_config = spawn_config.with_timeout(timeout);
    }
    if let Some(iterations) = max_iterations {
        spawn_config = spawn_config.with_max_iterations(iterations);
    }
    if wait_for_completion {
        spawn_config = spawn_config.synchronous();
    }

    // 创建 Sub-Agent
    let id = manager
        .spawn(&name, &role, &task, Some(spawn_config.clone()), parent_id)
        .await?;

    // 如果是异步模式，直接返回 ID
    if !wait_for_completion {
        return Ok(serde_json::json!({
            "success": true,
            "subagent_id": id.to_string(),
            "message": format!("Sub-Agent '{}' created successfully. Use get_sub_agent_result to check status.", name)
        }));
    }

    // 同步模式：执行并等待结果
    let agent = manager.get(&id).await?;
    let context = SubAgentContext::from_agent(&agent);

    let executor = SubAgentExecutor::new(
        state,
        chat_server,
        headers,
        manager.clone(),
        available_tools,
        model,
    );

    // 获取配置的超时和迭代限制
    let config = manager.config();
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(config.default_timeout_secs));
    let max_iter = max_iterations.unwrap_or(config.default_max_iterations);

    // 获取取消令牌
    let cancel_token = manager
        .get_cancel_token(&id)
        .await
        .unwrap_or_else(CancellationToken::new);

    // 执行
    let result = executor
        .execute(&id, context, timeout, max_iter, &cancel_token, emitter)
        .await;

    match result {
        Ok(sub_result) => Ok(serde_json::json!({
            "success": true,
            "subagent_id": id.to_string(),
            "result": SubAgentResultSummary {
                success: sub_result.error.is_none(),
                output: Some(sub_result.output.clone()),
                error: sub_result.error.clone(),
                iterations: sub_result.metrics.total_iterations,
                duration_ms: sub_result.metrics.duration_ms,
            }
        })),
        Err(e) => Ok(serde_json::json!({
            "success": false,
            "subagent_id": id.to_string(),
            "error": e.to_string()
        })),
    }
}

/// 执行 get_sub_agent_result 工具
pub async fn execute_get_sub_agent_result(
    manager: Arc<SubAgentManager>,
    subagent_id: String,
    wait: bool,
    timeout_secs: Option<u64>,
) -> ServerResult<serde_json::Value> {
    let id = super::types::SubAgentId::from_string(&subagent_id);

    // 获取 Sub-Agent 信息
    let agent = manager.get(&id).await?;

    // 如果已经是终态，直接返回
    if agent.state.is_terminal() {
        return Ok(build_result_response(&agent));
    }

    // 如果不等待，返回当前状态
    if !wait {
        return Ok(serde_json::json!({
            "subagent_id": subagent_id,
            "state": agent.state.to_string(),
            "message": "Sub-Agent is still running. Use wait=true to wait for completion."
        }));
    }

    // 等待完成
    let timeout = Duration::from_secs(timeout_secs.unwrap_or(60));
    let start = Instant::now();

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        let agent = manager.get(&id).await?;

        if agent.state.is_terminal() {
            return Ok(build_result_response(&agent));
        }

        if start.elapsed() > timeout {
            return Ok(serde_json::json!({
                "subagent_id": subagent_id,
                "state": agent.state.to_string(),
                "error": "Timeout waiting for Sub-Agent completion"
            }));
        }
    }
}

/// 执行 cancel_sub_agent 工具
pub async fn execute_cancel_sub_agent(
    manager: Arc<SubAgentManager>,
    subagent_id: String,
    _reason: Option<String>,
) -> ServerResult<serde_json::Value> {
    let id = super::types::SubAgentId::from_string(&subagent_id);

    manager.cancel(&id).await?;

    let agent = manager.get(&id).await?;

    Ok(serde_json::json!({
        "success": true,
        "subagent_id": subagent_id,
        "state": agent.state.to_string()
    }))
}

/// 构建结果响应
fn build_result_response(agent: &super::types::SubAgent) -> serde_json::Value {
    let mut response = serde_json::json!({
        "subagent_id": agent.id.to_string(),
        "state": agent.state.to_string(),
    });

    if let Some(result) = &agent.result {
        response["result"] = serde_json::json!({
            "success": result.error.is_none(),
            "output": result.output,
            "error": result.error,
            "iterations": result.metrics.total_iterations,
            "duration_ms": result.metrics.duration_ms,
        });
    }

    response
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mcp_tool_name() {
        let result = parse_mcp_tool_name("mcp__server__tool");
        assert_eq!(result, Some(("server", "tool")));

        let result = parse_mcp_tool_name("mcp__my_server__my_tool");
        assert_eq!(result, Some(("my_server", "my_tool")));

        let result = parse_mcp_tool_name("internal__spawn_sub_agent");
        assert_eq!(result, None);

        let result = parse_mcp_tool_name("invalid_format");
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_thought() {
        let content = "Some text <thought>I should search for this</thought> more text";
        let thought = extract_thought(content);
        assert_eq!(thought, Some("I should search for this".to_string()));

        let content = "No thought tag here";
        let thought = extract_thought(content);
        assert_eq!(thought, None);

        let content = "<thought>  Trimmed thought  </thought>";
        let thought = extract_thought(content);
        assert_eq!(thought, Some("Trimmed thought".to_string()));
    }

    #[test]
    fn test_extract_final_answer() {
        let content = "Analysis complete. <final_answer>The answer is 42.</final_answer>";
        let answer = extract_final_answer(content);
        assert_eq!(answer, Some("The answer is 42.".to_string()));

        let content = "No final answer here";
        let answer = extract_final_answer(content);
        assert_eq!(answer, None);
    }

    #[test]
    fn test_build_tools_json() {
        let tools = vec![ToolDescription {
            name: "mcp__server__tool".to_string(),
            description: "A test tool".to_string(),
        }];

        let json = build_tools_json(&tools);
        assert!(json.is_array());

        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "mcp__server__tool");
    }
}
