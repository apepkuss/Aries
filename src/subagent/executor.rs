//! Sub-Agent 执行器
//!
//! 此模块实现 SubAgentExecutor，负责执行 Sub-Agent 的任务。
//! 复用现有的 React 循环逻辑，但使用独立的上下文。

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use endpoints::chat::ChatCompletionObject;
use http::HeaderMap;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use tokio::{select, sync::RwLock};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::{
    context::SubAgentContext,
    manager::SubAgentManager,
    reflector::{ReflectionAction, SubAgentReflector},
    tools::{SubAgentResultSummary, is_subagent_tool},
    types::{SubAgentId, SubAgentMetrics, SubAgentResult},
};
use crate::{
    app::AppState,
    chat::{emitter::EventEmitter, events::ThoughtStatus, planner::ToolDescription},
    error::{ServerError, ServerResult},
    mcp::MCP_SERVICES,
    reflection::engine::LlmServerInfo,
    server::TargetServerInfo,
    services::hitl::{self, HitlError, HitlToolCaller, HitlToolContext, HitlToolResult},
    skills::{
        LoadedSkill, ScriptContext, SkillLoader,
        constants::{is_internal_tool, parse_internal_tool_name},
    },
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
    /// LLM 服务器信息（用于反思）
    llm_server: Option<Arc<RwLock<LlmServerInfo>>>,
    /// HITL 工具调用器（可选，如果启用 HITL）
    hitl_caller: Option<HitlToolCaller>,
    /// 会话 ID（用于 HITL 上下文）
    conversation_id: String,
    /// 用户 ID（用于 HITL 上下文）
    user_id: String,
    /// 当前活跃的 Skills（用于执行 internal__ 工具）
    active_skills: Vec<LoadedSkill>,
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
        Self::with_hitl_context(
            state,
            chat_server,
            headers,
            manager,
            available_tools,
            model,
            "default".to_string(),
            "anonymous".to_string(),
        )
        .with_active_skills(Vec::new())
    }

    /// 设置活跃的 Skills
    pub fn with_active_skills(mut self, skills: Vec<LoadedSkill>) -> Self {
        self.active_skills = skills;
        self
    }

    /// 创建带 HITL 上下文的执行器
    #[allow(clippy::too_many_arguments)]
    pub fn with_hitl_context(
        state: Arc<AppState>,
        chat_server: TargetServerInfo,
        headers: HeaderMap,
        manager: Arc<SubAgentManager>,
        available_tools: Vec<ToolDescription>,
        model: String,
        conversation_id: String,
        user_id: String,
    ) -> Self {
        // 创建 LLM 服务器信息用于反思
        let llm_server = Some(Arc::new(RwLock::new(LlmServerInfo {
            url: chat_server.url.clone(),
            api_key: chat_server.api_key.clone(),
        })));

        // 从全局 HITL Manager 创建 HitlToolCaller（如果已初始化且启用）
        let hitl_caller = hitl::global().map(|m| HitlToolCaller::new(Arc::clone(m)));

        Self {
            state,
            chat_server,
            headers,
            manager,
            available_tools,
            model,
            llm_server,
            hitl_caller,
            conversation_id,
            user_id,
            active_skills: Vec::new(),
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
    #[allow(clippy::too_many_arguments)]
    pub async fn execute(
        &self,
        id: &SubAgentId,
        mut context: SubAgentContext,
        timeout: Duration,
        max_iterations: u32,
        cancel_token: &CancellationToken,
        emitter: &dyn EventEmitter,
        plan_pause_tracker: Option<Arc<AtomicU64>>,
    ) -> ServerResult<SubAgentResult> {
        let start_time = Instant::now();

        // 标记为开始运行
        self.manager.mark_started(id).await?;

        // 过滤工具列表
        let filtered_tools = self.filter_tools_for_context(&context);

        // 创建反思器
        let reflection_config = self.manager.config().reflection.clone();
        let mut reflector = SubAgentReflector::new(reflection_config, self.llm_server.clone());

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
                &mut reflector,
                plan_pause_tracker,
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
        reflector: &mut SubAgentReflector,
        plan_pause_tracker: Option<Arc<AtomicU64>>,
    ) -> ServerResult<String> {
        // 获取任务描述用于反思
        let task_description = context.task_description().to_string();
        // 工具调用历史（用于反思上下文）
        let mut tool_history: Vec<String> = Vec::new();
        // 跟踪最后一次成功的工具调用结果（用于强制终止）
        let mut last_successful_tool_result: Option<String> = None;
        // Track total time spent in tool execution (including HITL wait).
        // Subtracted from elapsed time in the timeout check so that
        // human-in-the-loop approval time does not count toward the timeout.
        let mut tool_pause_duration = Duration::ZERO;

        loop {
            // 检查迭代限制
            context.increment_iteration();
            let iteration = context.current_iteration();

            if iteration > max_iterations {
                return Err(ServerError::MaxIterationsExceeded(max_iterations));
            }

            // 检查超时（排除工具执行/HITL 等待时间）
            let effective_elapsed = start_time.elapsed().saturating_sub(tool_pause_duration);
            if effective_elapsed > timeout {
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

            // Debug: 打印 LLM 响应
            debug!(
                "Sub-Agent LLM response: tool_calls={}, has_content={}, content_preview={}",
                tool_calls.len(),
                content.is_some(),
                content
                    .map(|c| if c.len() > 100 { &c[..100] } else { c })
                    .unwrap_or("(none)")
            );

            // 更新 token 使用量
            let prompt_tokens = chat_completion.usage.prompt_tokens;
            let completion_tokens = chat_completion.usage.completion_tokens;

            self.manager
                .update_metrics(id, |metrics| {
                    metrics.prompt_tokens += prompt_tokens;
                    metrics.completion_tokens += completion_tokens;
                })
                .await
                .ok();

            // 报告全局 Token 使用量
            self.manager
                .add_global_tokens(prompt_tokens, completion_tokens);

            // 检查全局 Token 限制
            if self.manager.is_token_limit_exceeded() {
                let max = self.manager.config().max_total_tokens;
                let used = self.manager.global_total_tokens();
                return Err(ServerError::SubAgentTokenLimitExceeded { used, max });
            }

            // 检查是否有工具调用
            if !tool_calls.is_empty() {
                // 如果之前已经有成功的工具调用，强制使用那个结果作为最终答案
                // 这防止 LLM 在收到工具结果后仍然调用更多工具
                if let Some(previous_result) = &last_successful_tool_result {
                    debug!(
                        "Sub-Agent attempted additional tool call after successful result. \
                         Forcing completion with previous result."
                    );
                    return Ok(previous_result.clone());
                }

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

                    // 发射 Sub-Agent 工具调用事件（而非主 Agent 的 tool_call）
                    let tool_args: serde_json::Value =
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::json!({}));

                    // 发送 Sub-Agent 专用的工具调用事件
                    emitter
                        .emit_subagent_tool_call(
                            id.as_ref(),
                            &tool_call.id,
                            &tool_call.function.name,
                            &tool_args,
                            Some(iteration),
                        )
                        .await;

                    // 发送进度事件（包含工具名，用于前端显示）
                    emitter
                        .emit_subagent_progress(
                            id.as_ref(),
                            iteration,
                            Some(max_iterations),
                            Some(&format!("Calling tool: {}", tool_call.function.name)),
                            Some(&tool_call.function.name),
                        )
                        .await;

                    // 执行工具调用（带 HITL 检查）
                    let tool_start = Instant::now();
                    let tool_result = self
                        .execute_tool_call(&tool_call.function.name, &tool_args, id, cancel_token)
                        .await;

                    let tool_duration = tool_start.elapsed();
                    // Exclude tool execution time (incl. HITL wait) from timeout
                    tool_pause_duration += tool_duration;
                    // Also report to plan-level time budget tracker
                    if let Some(ref tracker) = plan_pause_tracker {
                        tracker.fetch_add(tool_duration.as_nanos() as u64, Ordering::Relaxed);
                    }

                    // 记录工具调用历史
                    let tool_record = format!(
                        "Tool: {} | Args: {} | Duration: {:?}",
                        tool_call.function.name, tool_args, tool_duration
                    );

                    // 处理工具结果（不再发送 tool_result 事件到主事件流）
                    match &tool_result {
                        Ok(_result) => {
                            tool_history.push(format!("{} | Result: OK", tool_record));
                        }
                        Err(e) => {
                            // Check if this is a user interruption - terminate immediately without reflection
                            if matches!(e, ServerError::UserInterrupted(_)) {
                                return Err(e.clone());
                            }

                            let error_msg = e.to_string();
                            tool_history.push(format!("{} | Error: {}", tool_record, error_msg));

                            // 工具错误后的反思
                            let action = reflector
                                .reflect_on_tool_error(
                                    &task_description,
                                    &tool_call.function.name,
                                    &error_msg,
                                    &tool_history,
                                )
                                .await
                                .unwrap_or(ReflectionAction::Continue);

                            // 处理反思动作
                            if let ReflectionAction::Abort { reason } = action {
                                return Err(ServerError::Operation(format!(
                                    "Sub-Agent aborted after tool error reflection: {}",
                                    reason
                                )));
                            }

                            // 如果反思建议继续，则传播原始错误
                            // （让 LLM 有机会从错误中恢复）
                        }
                    }

                    let tool_result = tool_result?;

                    // 保存成功的工具结果，用于防止重复调用
                    last_successful_tool_result = Some(tool_result.clone());

                    // 添加助手消息和工具结果到上下文
                    context.add_assistant_message(
                        message.content.clone(),
                        Some(vec![tool_call.clone()]),
                    );

                    // 添加工具结果，并强调必须立即给出最终答案
                    let observation = format!(
                        "<observation>{}</observation>\n\n\
                        **IMPORTANT**: You have received the tool result above. \
                        You MUST now provide your final answer using `<final_answer>` tags. \
                        Do NOT call any more tools. Respond with:\n\
                        <final_answer>\nYour answer based on the tool result\n</final_answer>",
                        tool_result
                    );
                    context.add_tool_result(&observation, &tool_call.id);
                }

                // 周期性反思检查（在处理完工具调用后）
                let current_progress = format!(
                    "Iteration {}: Processed {} tool call(s). Last tool: {}",
                    iteration,
                    tool_calls.len(),
                    tool_calls
                        .last()
                        .map(|t| t.function.name.as_str())
                        .unwrap_or("none")
                );

                let action = reflector
                    .reflect_on_iteration(
                        iteration,
                        &task_description,
                        &current_progress,
                        &tool_history,
                    )
                    .await
                    .unwrap_or(ReflectionAction::Continue);

                // 处理周期性反思动作
                if let ReflectionAction::Abort { reason } = action {
                    return Err(ServerError::Operation(format!(
                        "Sub-Agent aborted after periodic reflection: {}",
                        reason
                    )));
                }

                // 如果有指导建议，添加到上下文
                if let Some(guidance) = action.guidance() {
                    debug!("Reflection guidance: {}", guidance);
                    // 可以选择将指导添加到上下文中
                    // context.add_system_hint(guidance);
                }
            } else {
                // 没有工具调用，检查是否有最终答案
                if let Some(content) = content {
                    // 检查是否包含最终答案
                    if let Some(answer) = extract_final_answer(content) {
                        // 完成时的反思
                        let action = reflector
                            .reflect_on_completion(
                                iteration,
                                &task_description,
                                &answer,
                                &tool_history,
                            )
                            .await
                            .unwrap_or(ReflectionAction::Accept);

                        // 处理完成反思动作
                        match action {
                            ReflectionAction::Abort { reason } => {
                                return Err(ServerError::Operation(format!(
                                    "Sub-Agent answer rejected by reflection: {}",
                                    reason
                                )));
                            }
                            ReflectionAction::Retry { reason }
                            | ReflectionAction::RetryWithGuidance { guidance: reason } => {
                                // 如果可以重试且反思建议重试
                                if reflector.can_retry() {
                                    debug!("Reflection suggests retry: {}", reason);
                                    // 添加提示让 LLM 重新考虑
                                    context.add_assistant_message(Some(content.clone()), None);
                                    context.add_tool_result(
                                        &format!(
                                            "<reflection_feedback>Please reconsider your answer. {}</reflection_feedback>",
                                            reason
                                        ),
                                        "reflection",
                                    );
                                    continue; // 继续循环，让 LLM 重新生成
                                }
                                // 超过重试限制，接受当前答案
                            }
                            _ => {
                                // Accept, AcceptWithGuidance, Continue - 接受答案
                            }
                        }

                        return Ok(answer);
                    }

                    // 如果内容不为空且没有工具调用，视为最终响应
                    if !content.trim().is_empty() {
                        // 对非结构化答案也进行完成反思
                        let action = reflector
                            .reflect_on_completion(
                                iteration,
                                &task_description,
                                content,
                                &tool_history,
                            )
                            .await
                            .unwrap_or(ReflectionAction::Accept);

                        if let ReflectionAction::Abort { reason } = action {
                            return Err(ServerError::Operation(format!(
                                "Sub-Agent response rejected by reflection: {}",
                                reason
                            )));
                        }

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

        // Debug: 打印工具列表
        debug!(
            "Sub-Agent LLM request: {} tools available, tool names: {:?}",
            available_tools.len(),
            available_tools.iter().map(|t| &t.name).collect::<Vec<_>>()
        );

        // 构建请求
        // 如果有工具可用，设置 tool_choice 为 "auto" 以鼓励模型使用工具
        let request_json = if available_tools.is_empty() {
            serde_json::json!({
                "model": self.model,
                "messages": context.messages(),
                "stream": false
            })
        } else {
            serde_json::json!({
                "model": self.model,
                "messages": context.messages(),
                "tools": tools_json,
                "tool_choice": "auto",
                "stream": false
            })
        };

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

    /// 执行工具调用（带 HITL 检查）
    async fn execute_tool_call(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        subagent_id: &SubAgentId,
        cancel_token: &CancellationToken,
    ) -> ServerResult<String> {
        // 检查是否是 Sub-Agent 工具（不允许递归调用，除非配置允许）
        if is_subagent_tool(tool_name) {
            return Err(ServerError::Operation(
                "Sub-Agent cannot spawn another Sub-Agent by default".to_string(),
            ));
        }

        // 如果启用了 HITL，使用 HitlToolCaller 进行检查
        if let Some(ref hitl_caller) = self.hitl_caller {
            // 从 manager 获取 SubAgent 以获取 subtask_id
            let subtask_id = self
                .manager
                .get(subagent_id)
                .await
                .ok()
                .and_then(|sa| sa.subtask_id);

            let mut context = HitlToolContext::new(&self.conversation_id, &self.user_id)
                .with_subagent_id(subagent_id.to_string())
                .with_cancel_token(cancel_token.clone());

            // 如果有 subtask_id，添加到上下文
            if let Some(id) = subtask_id {
                context = context.with_subtask_id(id);
            }

            let tool_name_clone = tool_name.to_string();
            let active_skills = self.active_skills.clone();
            let conv_id = self.conversation_id.clone();
            let result = hitl_caller
                .check_and_execute(tool_name, args, &context, |args| {
                    let tool_name = tool_name_clone.clone();
                    let skills = active_skills.clone();
                    let conv_id = conv_id.clone();
                    async move {
                        Self::dispatch_tool_execution(&tool_name, &args, &skills, &conv_id).await
                    }
                })
                .await;

            match result {
                Ok(hitl_result) => match hitl_result {
                    HitlToolResult::Executed(r)
                    | HitlToolResult::ExecutedWithoutConfirmation(r)
                    | HitlToolResult::Approved(r)
                    | HitlToolResult::HitlDisabled(r) => Ok(r),
                    HitlToolResult::Modified { result, .. } => Ok(result),
                    HitlToolResult::Rejected { reason } => {
                        Err(ServerError::UserInterrupted(reason.unwrap_or_else(|| {
                            "Tool call rejected by user".to_string()
                        })))
                    }
                    HitlToolResult::Skipped { reason } => Err(ServerError::Operation(format!(
                        "Tool call skipped: {}",
                        reason
                    ))),
                    HitlToolResult::Aborted { reason } => Err(ServerError::Operation(format!(
                        "Tool call aborted: {}",
                        reason.unwrap_or_else(|| "No reason provided".to_string())
                    ))),
                    HitlToolResult::TimedOut { behavior } => Err(ServerError::Operation(format!(
                        "HITL confirmation timed out (behavior: {})",
                        behavior
                    ))),
                },
                Err(e) => {
                    // Handle Cancelled error specially - treat it as user interruption
                    // for fail-fast behavior to work correctly
                    if matches!(e, HitlError::Cancelled(_)) {
                        Err(ServerError::UserInterrupted(
                            "HITL request cancelled due to another subtask rejection".to_string(),
                        ))
                    } else {
                        Err(ServerError::Operation(format!("HITL error: {}", e)))
                    }
                }
            }
        } else {
            // 没有 HITL，直接执行
            Self::dispatch_tool_execution(
                tool_name,
                args,
                &self.active_skills,
                &self.conversation_id,
            )
            .await
            .map_err(ServerError::Operation)
        }
    }

    /// 统一工具执行分发：根据工具名称前缀路由到 internal 或 MCP 执行路径
    async fn dispatch_tool_execution(
        tool_name: &str,
        args: &serde_json::Value,
        active_skills: &[LoadedSkill],
        conversation_id: &str,
    ) -> Result<String, String> {
        if is_internal_tool(tool_name) {
            Self::execute_internal_tool(tool_name, args, active_skills, conversation_id).await
        } else {
            Self::execute_mcp_tool(tool_name, args).await
        }
    }

    /// 执行 internal 工具调用（skill_run_script, skill_load_asset）
    async fn execute_internal_tool(
        full_tool_name: &str,
        args: &serde_json::Value,
        active_skills: &[LoadedSkill],
        conversation_id: &str,
    ) -> Result<String, String> {
        let tool_name = parse_internal_tool_name(full_tool_name)
            .ok_or_else(|| format!("Invalid internal tool name: {full_tool_name}"))?;

        match tool_name {
            "skill_run_script" => {
                let skill = active_skills.first().ok_or_else(|| {
                    "skill_run_script requires an active skill. No skill is active for this Sub-Agent.".to_string()
                })?;

                let script_name = args
                    .get("script_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        "skill_run_script requires 'script_name' argument".to_string()
                    })?;

                let script_args: Vec<String> = args
                    .get("args")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let context = ScriptContext::with_ids(Some(conversation_id.to_string()), None);

                match skill
                    .execute_script_with_context(script_name, script_args, context, None)
                    .await
                {
                    Ok(output) => {
                        if output.exit_code == 0 {
                            Ok(format!(
                                "Script '{}' executed successfully.\n\nOutput:\n{}",
                                script_name,
                                output.stdout.trim()
                            ))
                        } else {
                            Ok(format!(
                                "Script '{}' failed with exit code {}.\n\nStdout:\n{}\n\nStderr:\n{}",
                                script_name,
                                output.exit_code,
                                output.stdout.trim(),
                                output.stderr.trim()
                            ))
                        }
                    }
                    Err(e) => Err(format!("Script execution failed: {}", e)),
                }
            }
            "skill_load_asset" => {
                let skill = active_skills.first().ok_or_else(|| {
                    "skill_load_asset requires an active skill. No skill is active for this Sub-Agent.".to_string()
                })?;

                let asset_name = args
                    .get("asset_name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "skill_load_asset requires 'asset_name' argument".to_string())?;

                let content = SkillLoader::load_asset_string(&skill.skill_dir, asset_name)
                    .await
                    .ok_or_else(|| {
                        format!(
                            "Asset '{}' not found in skill '{}'",
                            asset_name, skill.metadata.name
                        )
                    })?;

                // Apply template variable replacement if variables provided
                let content = if let Some(vars) = args.get("variables").and_then(|v| v.as_object())
                {
                    let mut result = content;
                    for (key, value) in vars {
                        let placeholder = format!("{{{{{}}}}}", key);
                        let replacement = match value {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        result = result.replace(&placeholder, &replacement);
                    }
                    result
                } else {
                    content
                };

                Ok(format!(
                    "Asset '{}' loaded successfully.\n\nContent:\n{}",
                    asset_name, content
                ))
            }
            _ => Err(format!("Unknown internal tool: {tool_name}")),
        }
    }

    /// 执行 MCP 工具调用（内部方法，不经过 HITL）
    async fn execute_mcp_tool(tool_name: &str, args: &serde_json::Value) -> Result<String, String> {
        // 解析 MCP 工具名称
        let (server_name, mcp_tool_name) = parse_mcp_tool_name(tool_name)
            .ok_or_else(|| format!("Invalid tool name format: {tool_name}"))?;

        // 获取 MCP 服务
        let services = MCP_SERVICES
            .get()
            .ok_or_else(|| "MCP services not initialized".to_string())?;

        let service_map = services.read().await;
        let service = service_map
            .get(server_name)
            .ok_or_else(|| format!("MCP server '{}' not found", server_name))?;

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
            .map_err(|e| format!("Tool call failed: {e}"))?;

        if result.is_error == Some(true) {
            let error_detail = result
                .content
                .first()
                .and_then(|c| match &c.raw {
                    rmcp::model::RawContent::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .unwrap_or_else(|| "Unknown error".to_string());
            return Err(format!("Tool returned error: {}", error_detail));
        }

        // 提取结果文本
        if !result.content.is_empty()
            && let rmcp::model::RawContent::Text(text) = &result.content[0].raw
        {
            return Ok(text.text.clone());
        }

        Err("MCP tool returned empty content".to_string())
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
///
/// 使用工具的实际参数架构（如果有），否则使用空的对象架构
fn build_tools_json(tools: &[ToolDescription]) -> serde_json::Value {
    let tools_json: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            // 使用工具的实际参数架构，或者默认的空对象架构
            let parameters = tool.parameters.clone().unwrap_or_else(|| {
                serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                })
            });

            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": parameters
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
        .execute(
            &id,
            context,
            timeout,
            max_iter,
            &cancel_token,
            emitter,
            None,
        )
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
            ..Default::default()
        }];

        let json = build_tools_json(&tools);
        assert!(json.is_array());

        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["type"], "function");
        assert_eq!(arr[0]["function"]["name"], "mcp__server__tool");
    }
}
