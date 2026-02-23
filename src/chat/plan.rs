//! Plan mode chat handler.
//!
//! This module implements the Plan mode, which decomposes user requests into
//! subtasks and executes them according to a dependency-aware execution order.
//! Each subtask is executed using a React loop for iterative reasoning.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime},
};

use axum::{
    Json,
    body::Body,
    extract::{Extension, State},
    http::{HeaderMap, Response, StatusCode},
};
use endpoints::chat::{
    ChatCompletionAssistantMessage, ChatCompletionChunk, ChatCompletionChunkChoice,
    ChatCompletionChunkChoiceDelta, ChatCompletionObject, ChatCompletionRequest,
    ChatCompletionRequestBuilder, ChatCompletionRequestMessage, ChatCompletionRole,
    ChatCompletionSystemMessage, ChatCompletionToolMessage, ChatCompletionUserMessage,
    ChatCompletionUserMessageContent,
};
use futures_util::stream::{self, StreamExt};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use rmcp::model::{CallToolRequestParam, RawContent};
use tokio::{select, sync::mpsc};
use tokio_util::sync::CancellationToken;

use super::{
    emitter::{EventEmitter, create_emitter},
    events::{EnhancedStreamConfig, ExecutionPhase, ExecutionSummary, ThoughtStatus},
    shared::TimeBudget,
    trace::{IterationTrace, ToolCallTrace},
    xml_parser::{
        PlannerOutput, extract_action, extract_final_answer, extract_thought,
        extract_xml_tool_call, has_action_tag, has_final_answer_tag,
    },
};
use crate::{
    AppState,
    chat::{
        gen_chat_id,
        planner::{PlannerMessage, SubTask, SubTaskStatus, TaskPlan, TaskPlanner, ToolDescription},
        trace::{
            PlanTrace, ReplanEvent, SubtaskReflectionSummary, SubtaskTrace, TokenUsage, TraceStatus,
        },
        utils::*,
    },
    dual_debug, dual_error, dual_info, dual_warn,
    error::{ServerError, ServerResult},
    hitl::{self, HitlToolCaller, HitlToolContext, HitlToolResult},
    mcp::{
        DEFAULT_SEARCH_FALLBACK_MESSAGE, MCP_SERVICES, SEARCH_MCP_SERVER_NAMES, extract_tool_name,
        format_mcp_tool_name, parse_mcp_tool_name,
    },
    reflection::{
        AdaptiveStrategy, CacheConfig, DependencyGraph, DynamicReplanner, FailedSubtaskInfo,
        LlmServerInfo, RecommendedAction, ReflectionCache, ReflectionContext, ReflectionEngine,
        ReplanContext, ReplanTrigger, SubtaskInfo,
    },
    server::{RoutingPolicy, ServerKind},
    services::hitl::types::{DetectedPrivacyPattern, HitlResponse},
    skills::{
        LoadedSkill, ScriptContext, SkillDetector, SkillInjector, SkillLoader, SkillRegistry,
        SkillSummary,
        constants::{
            INTERNAL_TOOL_PREFIX, LANTAI_DELETE_MEMORY_TOOL, LANTAI_SEARCH_TOOL, LANTAI_STATS_TOOL,
            LANTAI_UPDATE_MEMORY_TOOL, LANTAI_WRITE_MEMORY_TOOL, SKILL_LOAD_ASSET_TOOL,
            SKILL_RUN_SCRIPT_TOOL, internal_tool_name, is_internal_tool, parse_internal_tool_name,
        },
    },
    subagent::{
        self, CANCEL_SUB_AGENT_TOOL, CancelSubAgentArgs, GET_SUB_AGENT_RESULT_TOOL,
        GetSubAgentResultArgs, RateLimiter, SPAWN_SUB_AGENT_TOOL, SpawnSubAgentArgs,
        SubAgentContext, SubAgentExecutor, SubAgentManager, SubAgentSpawnConfig,
        SubAgentSystemConfig, SubAgentToolAccess, all_subagent_tool_descriptions, is_subagent_tool,
        parse_subagent_tool_name,
    },
};

// ============================================================================
// Plan Mode Handler
// ============================================================================

/// Main entry point for Plan mode chat handling.
pub(crate) async fn chat(
    State(state): State<Arc<AppState>>,
    Extension(cancel_token): Extension<CancellationToken>,
    mut headers: HeaderMap,
    Json(mut request): Json<ChatCompletionRequest>,
    conv_id: Option<String>,
    session_id: Option<String>,
    request_id: impl AsRef<str>,
) -> ServerResult<axum::response::Response> {
    let request_id = request_id.as_ref();

    // Extract user message (with file attachments) for planning and privacy detection
    let (user_message, file_attachments) = extract_user_message_with_files(&request);

    // Extract system message for memory storage
    let system_message = extract_system_message(&request);

    // Get user ID from request
    let user_id = request.user.as_deref().unwrap_or("anonymous");

    // Determine server kind with smart privacy detection
    // This checks X-Privacy-Mode header first, then runs privacy detection if needed
    let effective_conv_id = conv_id.as_deref().unwrap_or("default");
    let server_kind = detect_and_confirm_privacy(
        &state,
        &headers,
        user_message.as_deref(),
        effective_conv_id,
        user_id,
        &cancel_token,
    )
    .await?;

    // If smart detection determined privacy mode, set the header for downstream consistency
    // This ensures that background tasks (like execute_chat_plan_realtime) see the correct mode
    if server_kind == ServerKind::privacy_chat
        && headers
            .get("x-privacy-mode")
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| v != "true")
    {
        headers.insert(
            "x-privacy-mode",
            axum::http::HeaderValue::from_static("true"),
        );
    }

    let chat_server = get_chat_server(&state, request_id, server_kind).await?;
    let is_privacy = server_kind == ServerKind::privacy_chat;

    // Store the latest user message to memory
    if let Some(memory) = &state.memory
        && let Some(conv_id) = &conv_id
        && let Some(user_msg) = &user_message
    {
        // Handle system message storage
        if let Some(sys_msg) = &system_message
            && let Ok(updated) = memory.set_system_message(conv_id, sys_msg).await
            && updated
        {
            dual_debug!(
                "System message updated for conversation {} - request_id: {}",
                conv_id,
                request_id
            );
        }

        // Store user message
        if let Err(e) = memory
            .add_user_message(conv_id, user_msg.clone(), is_privacy)
            .await
        {
            dual_error!(
                "Failed to add user message to memory: {} - request_id: {}",
                e,
                request_id
            );
        }
    }

    // Write user message to session history (JSONL)
    if let Some(ref sid) = session_id
        && let Some(ref writer) = state.session_writer
        && let Some(ref user_msg) = user_message
    {
        let uid = request.user.as_deref().unwrap_or("anonymous");
        let model_name = request
            .model
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let seq = writer.next_sequence(uid).await;
        let record = crate::session::types::SessionRecord::Message {
            version: crate::session::types::JSONL_FORMAT_VERSION,
            role: "user".to_string(),
            content: user_msg.clone(),
            timestamp: chrono::Utc::now(),
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            sequence: seq,
            tokens: None,
            tool_calls: None,
            privacy_mode: is_privacy,
        };
        if let Err(e) = writer.append_message(uid, sid, &model_name, record).await {
            dual_warn!(
                "Failed to write user message to session: {} - request_id: {}",
                e,
                request_id
            );
        }
    }

    // Get plan mode configuration
    let (
        max_plan_subtasks,
        plan_timeout_secs,
        subtask_max_retries,
        subtask_react_max_iterations,
        subtask_react_timeout_secs,
        max_tools_per_iteration,
        tool_call_max_retries,
        tool_call_retry_delay_ms,
        reflection_config,
        subagent_config,
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
            config.subagent.clone(),
        )
    };

    // Determine execution mode for subtasks
    let use_subagent_execution = subagent_config
        .as_ref()
        .map(|c| c.is_subagent_mode())
        .unwrap_or(false);

    // Log the execution mode for debugging
    let execution_mode_str = subagent_config
        .as_ref()
        .map(|c| c.execution_mode.as_str())
        .unwrap_or("direct (default)");
    dual_info!(
        "🎯 Execution mode: {} (use_subagent={}) - request_id: {}",
        execution_mode_str,
        use_subagent_execution,
        request_id
    );

    // Initialize time budget for the entire plan
    let time_budget = TimeBudget::new(plan_timeout_secs);

    // Initialize reflection system (if enabled)
    let reflection_model = request
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());
    let reflection_engine = if reflection_config.enabled {
        let server_info = Arc::new(tokio::sync::RwLock::new(LlmServerInfo {
            url: chat_server.url.clone(),
            api_key: chat_server.api_key.clone(),
            model: reflection_model.clone(),
        }));
        Some(ReflectionEngine::new(
            server_info,
            reflection_config.clone(),
        ))
    } else {
        None
    };

    // Initialize reflection cache (if reflection is enabled)
    let reflection_cache = if reflection_config.enabled {
        Some(ReflectionCache::new(CacheConfig::default()))
    } else {
        None
    };

    // Initialize adaptive strategy (if reflection is enabled)
    let adaptive_strategy = if reflection_config.enabled {
        Some(AdaptiveStrategy::new(Default::default()))
    } else {
        None
    };

    // Initialize dynamic replanner (if reflection is enabled)
    let dynamic_replanner = if reflection_config.enabled {
        let server_info = Arc::new(tokio::sync::RwLock::new(LlmServerInfo {
            url: chat_server.url.clone(),
            api_key: chat_server.api_key.clone(),
            model: reflection_model.clone(),
        }));
        Some(DynamicReplanner::with_defaults(server_info))
    } else {
        None
    };

    if reflection_config.enabled {
        dual_info!("🔍 Reflection system enabled - request_id: {}", request_id);
    }

    // Store original stream setting
    let stream = request.stream.unwrap_or(false);
    request.stream = Some(false);

    // Parse enhanced stream configuration from headers
    let enhanced_stream_config = EnhancedStreamConfig::from_headers(&headers);
    if enhanced_stream_config.enabled {
        dual_info!(
            "🔄 Enhanced streaming enabled (thoughts={}, tools={}, status={}) - request_id: {}",
            enhanced_stream_config.include_thoughts,
            enhanced_stream_config.include_tool_calls,
            enhanced_stream_config.include_status,
            request_id
        );
    }

    // Check if realtime streaming mode should be used
    // Realtime streaming sends events immediately as they occur, rather than batching
    if stream && enhanced_stream_config.enabled {
        dual_info!(
            "🚀 Using realtime streaming mode - request_id: {}",
            request_id
        );
        return chat_realtime_stream(
            state,
            cancel_token,
            headers,
            request,
            conv_id,
            session_id,
            request_id.to_string(),
            enhanced_stream_config,
            time_budget,
            reflection_engine,
            reflection_cache,
            adaptive_strategy,
            dynamic_replanner,
            reflection_config,
            file_attachments,
        )
        .await;
    }

    // Create event emitter for structured event streaming (batch mode)
    // Channel capacity of 256 should be enough for most agent executions
    let (emitter, event_receiver) = create_emitter(&enhanced_stream_config, 256);

    // ========================================================================
    // Phase 1: Task Planning
    // ========================================================================

    dual_info!("📋 Starting task planning - request_id: {}", request_id);

    // Emit planning status event
    emitter
        .emit_status(
            ExecutionPhase::Planning,
            "Analyzing user request and generating task plan...",
            None,
            None,
            None,
        )
        .await;

    let user_request = user_message.clone().unwrap_or_default();

    // Get available tools from MCP services (with full parameter schemas)
    let available_tools = get_available_tools(&state).await;

    // Get available Skills summaries from global registry
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
        Err(_) => {
            // Skills registry not initialized, continue without skills
            dual_debug!("Skills registry not available - request_id: {}", request_id);
            vec![]
        }
    };

    // Get model name from request, fallback to "default" if not specified
    let model_name = request
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());

    // Read recent conversation history from Session JSONL for multi-turn context
    let chat_history: Vec<PlannerMessage> = if let Some(ref sid) = session_id
        && let Some(ref writer) = state.session_writer
    {
        let uid = request.user.as_deref().unwrap_or("anonymous");
        let reader = crate::session::reader::SessionReader::new(writer.base_dir());
        match reader.read_session(uid, sid).await {
            Ok(records) => {
                let all_messages: Vec<PlannerMessage> = records
                    .into_iter()
                    .filter_map(|r| match r {
                        crate::session::types::SessionRecord::Message {
                            role,
                            content,
                            privacy_mode,
                            ..
                        } => {
                            if privacy_mode {
                                return None;
                            }
                            match role.as_str() {
                                "user" => Some(PlannerMessage::user(content)),
                                "assistant" => Some(PlannerMessage::assistant(content)),
                                _ => None,
                            }
                        }
                        _ => None,
                    })
                    .collect();
                // Skip the last message (current user message already written to JSONL)
                // and take up to 10 recent messages
                let len = all_messages.len();
                if len > 1 {
                    let start = (len - 1).saturating_sub(10);
                    all_messages[start..len - 1].to_vec()
                } else {
                    vec![]
                }
            }
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    // Create task planner
    let mut planner = TaskPlanner::with_chat_llm(
        format!("{}/chat/completions", chat_server.url.trim_end_matches('/')),
        chat_server.api_key.clone(),
        model_name.clone(),
        max_plan_subtasks,
    )
    .with_tools(available_tools.clone())
    .with_skills(skills_summaries.clone())
    .with_chat_history(chat_history);

    // Add memory guidance and context when memory tools are available
    if state.has_memory_writer() {
        let mut memory_rules = vec![
            "You have access to a personal knowledge base with memory tools (lantai_write_memory, lantai_search, etc.). Use TaskPlan with memory tools ONLY when the user explicitly asks to save, remember, recall, or look up PERSONAL information (their preferences, decisions, notes, past instructions). For general knowledge questions (facts, geography, science, history, common knowledge, etc.), use DirectAnswer — do NOT route them through memory tools.".to_string(),
        ];

        // Inject recent memory context into planner so DirectAnswer can leverage past knowledge
        let lantai_cfg = state.config.read().await.lantai.clone();
        let context_injection_enabled = lantai_cfg
            .as_ref()
            .map(|l| l.auto_memory.context_injection)
            .unwrap_or(true);
        if context_injection_enabled && let Some(writer) = state.memory_writer() {
            let max_chars = lantai_cfg
                .as_ref()
                .map(|l| l.auto_memory.max_context_chars)
                .unwrap_or(2000);
            let memory_context = load_memory_context(writer, max_chars).await;
            if !memory_context.is_empty() {
                memory_rules.push(format!(
                    "\n## Recent Memory Context\n{memory_context}\n\nUse this context to inform your responses when relevant."
                ));
            }
        }

        planner = planner.with_extra_rules(memory_rules);
    }

    // Generate task plan or direct answer
    let planner_output = match planner.plan(&user_request).await {
        Ok(output) => output,
        Err(e) => {
            dual_error!(
                "Failed to generate task plan: {} - request_id: {}",
                e,
                request_id
            );
            return Err(e);
        }
    };

    // Handle direct answer case - return immediately without task execution
    let mut plan = match planner_output {
        PlannerOutput::DirectAnswer(answer) => {
            dual_info!("📝 Direct answer mode - request_id: {}", request_id);

            // Store assistant response to memory if available
            if let Some(memory) = &state.memory
                && let Some(conv_id) = &conv_id
                && let Err(e) = memory
                    .add_assistant_message(conv_id, &answer.answer, vec![], is_privacy)
                    .await
            {
                dual_error!(
                    "Failed to add assistant message to memory: {} - request_id: {}",
                    e,
                    request_id
                );
            }

            // Write assistant message to session history
            write_assistant_to_session(&state, &session_id, &request, &answer.answer, is_privacy)
                .await;

            // Trigger async memory recording for direct answer (fire-and-forget)
            if !is_privacy && state.has_memory_writer() {
                let state_clone = Arc::clone(&state);
                let chat_url =
                    format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
                let api_key = chat_server.api_key.clone();
                let model = model_name.clone();
                let user_msg = user_request.clone();
                let assistant_msg = answer.answer.clone();
                tokio::spawn(async move {
                    trigger_direct_answer_memory(
                        &state_clone,
                        &chat_url,
                        api_key.as_deref(),
                        &model,
                        &user_msg,
                        &assistant_msg,
                    )
                    .await;
                });
            }

            // Build and return response directly
            return build_direct_answer_response(
                &answer.answer,
                request_id,
                request.stream.unwrap_or(false),
            );
        }
        PlannerOutput::TaskPlan(raw_plan) => {
            // Convert raw plan to validated TaskPlan
            TaskPlan::from_raw(raw_plan)?
        }
    };

    dual_info!(
        "📋 Task plan generated: {} subtasks - request_id: {}",
        plan.len(),
        request_id
    );

    // Log the plan
    for (i, subtask) in plan.subtasks.iter().enumerate() {
        let skill_info = subtask
            .recommended_skill
            .as_ref()
            .map(|s| format!(", skill: {}", s))
            .unwrap_or_default();
        dual_debug!(
            "  Subtask {}: {} (deps: {:?}, tools: {:?}{})",
            subtask.id,
            subtask.description,
            subtask.dependencies,
            subtask.required_tools,
            skill_info
        );
        if i < plan.execution_order.len() {
            dual_debug!("  Execution order[{}]: {}", i, plan.execution_order[i]);
        }
    }

    // Initialize execution trace
    let mut trace = PlanTrace::new(
        request_id.to_string(),
        plan.original_goal.clone(),
        plan.execution_order.clone(),
    );
    trace.start();

    // ========================================================================
    // Phase 2: Task Execution (React Loop per Subtask)
    // ========================================================================

    dual_info!("🚀 Starting task execution - request_id: {}", request_id);

    // Extract user_id from headers for HITL context
    let user_id = headers
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    // Emit executing status event with subtask count
    let total_subtasks = plan.execution_order.len();
    emitter
        .emit_status(
            ExecutionPhase::Executing,
            &format!("Executing {} subtasks...", total_subtasks),
            None,
            Some(0),
            Some(total_subtasks),
        )
        .await;

    let mut completed_subtasks: HashSet<usize> = HashSet::new();
    let mut subtask_results: Vec<(usize, String)> = Vec::new();

    // Build dependency graph for critical subtask detection (R5.2)
    #[allow(unused_variables)]
    let dependency_graph = DependencyGraph::from_subtask_dependencies(
        &plan.subtasks.iter().map(|s| s.id).collect::<Vec<_>>(),
        &plan
            .subtasks
            .iter()
            .map(|s| s.dependencies.clone())
            .collect::<Vec<_>>(),
    );

    // Calculate pending subtask count for time allocation
    let mut pending_count = plan.execution_order.len();

    // Replan tracking (for R5.2 dynamic replanner integration)
    #[allow(unused_variables, unused_mut)]
    let mut replan_count = 0u32;
    #[allow(dead_code)]
    const MAX_REPLAN_ATTEMPTS: u32 = 3;

    // Check if parallel execution should be used
    let use_parallel_execution =
        should_use_parallel_execution(subagent_config.as_ref(), plan.execution_order.len());

    if use_parallel_execution {
        dual_info!(
            "🚀 Using parallel execution for {} subtasks - request_id: {}",
            plan.execution_order.len(),
            request_id
        );

        // Create parallel execution context
        let parallel_ctx = ParallelExecutionContext {
            state: state.clone(),
            chat_server: chat_server.clone(),
            headers: headers.clone(),
            available_tools: available_tools.clone(),
            skills_summaries: skills_summaries.clone(),
            cancel_token: cancel_token.clone(),
            request_id: request_id.to_string(),
            model_name: model_name.clone(),
            subagent_config: subagent_config.clone(),
            rate_limiter: None, // Rate limiter can be added if needed
            emitter: emitter.clone(),
            file_attachments: file_attachments.clone(),
        };

        // Execute subtasks in parallel
        execute_subtasks_parallel(
            &parallel_ctx,
            &mut plan.subtasks,
            &mut subtask_results,
            &mut completed_subtasks,
            &time_budget,
            emitter.as_ref(),
            &mut trace,
            total_subtasks,
            time_budget.pause_tracker(),
        )
        .await?;
    } else {
        // Sequential execution (existing code)
        dual_info!(
            "📋 Using sequential execution for {} subtasks - request_id: {}",
            plan.execution_order.len(),
            request_id
        );

        'execution: loop {
            let execution_order = plan.execution_order.clone();

            for &subtask_idx in &execution_order {
                // Check time budget
                if time_budget.is_exhausted() {
                    dual_warn!(
                        "Plan time budget exhausted after {} seconds - request_id: {}",
                        time_budget.elapsed().as_secs(),
                        request_id
                    );
                    trace.finalize(TraceStatus::Timeout);
                    dual_info!("Plan trace: {}", trace.summary());
                    return Err(ServerError::TimeBudgetExhausted {
                        elapsed_secs: time_budget.elapsed().as_secs(),
                    });
                }

                // Check cancellation
                if cancel_token.is_cancelled() {
                    let warn_msg = "Request was cancelled by client";
                    dual_warn!("{} - request_id: {}", warn_msg, request_id);
                    trace.finalize(TraceStatus::Error(warn_msg.to_string()));
                    return Err(ServerError::Operation(warn_msg.to_string()));
                }

                let subtask = match plan.subtasks.get_mut(subtask_idx) {
                    Some(s) => s,
                    None => continue,
                };

                // Initialize subtask trace
                let mut subtask_trace = SubtaskTrace::new(subtask.id, subtask.description.clone());

                dual_info!(
                    "▶️ Executing subtask {}: {} - request_id: {}",
                    subtask.id,
                    subtask.description,
                    request_id
                );

                // Emit status event for subtask start
                let subtask_current = completed_subtasks.len() + 1;
                emitter
                    .emit_status(
                        ExecutionPhase::Executing,
                        &format!("Executing subtask {}: {}", subtask.id, subtask.description),
                        Some(subtask.id),
                        Some(subtask_current),
                        Some(total_subtasks),
                    )
                    .await;

                // Check dependencies
                if !subtask.is_ready(&completed_subtasks) {
                    dual_warn!(
                        "Subtask {} has unmet dependencies, skipping - request_id: {}",
                        subtask.id,
                        request_id
                    );
                    subtask.skip();
                    subtask_trace.status = crate::chat::planner::SubTaskStatus::Skipped;
                    dual_info!("Subtask trace: {}", subtask_trace.summary());
                    trace.add_subtask_trace(subtask_trace);
                    pending_count = pending_count.saturating_sub(1);
                    continue;
                }

                // Start subtask execution
                subtask.start();
                subtask_trace.start();

                // Allocate time budget for this subtask (including potential retries)
                let subtask_time_budget = time_budget.allocate(pending_count);
                // Ensure we don't exceed the configured subtask timeout
                let effective_timeout =
                    subtask_time_budget.min(Duration::from_secs(subtask_react_timeout_secs));

                dual_debug!(
                    "Allocated {:?} for subtask {} (pending: {}, max_retries: {}) - request_id: {}",
                    effective_timeout,
                    subtask.id,
                    pending_count,
                    subtask_max_retries,
                    request_id
                );

                // Execute the subtask with retry loop
                let mut last_error: Option<ServerError> = None;
                let subtask_start_time = Instant::now();

                for attempt in 0..=subtask_max_retries {
                    // Check if we've exceeded the total time budget for this subtask
                    let elapsed = subtask_start_time.elapsed();
                    if elapsed >= subtask_time_budget {
                        dual_warn!(
                            "Subtask {} time budget exhausted after {:?} - request_id: {}",
                            subtask.id,
                            elapsed,
                            request_id
                        );
                        last_error = Some(ServerError::SubtaskTimeout {
                            subtask_id: subtask.id,
                            timeout_secs: subtask_time_budget.as_secs(),
                        });
                        break;
                    }

                    // Calculate remaining time for this attempt
                    let remaining_time = subtask_time_budget.saturating_sub(elapsed);
                    let attempt_timeout = remaining_time.min(effective_timeout);

                    if attempt > 0 {
                        dual_info!(
                            "🔄 Retrying subtask {} (attempt {}/{}) - request_id: {}",
                            subtask.id,
                            attempt + 1,
                            subtask_max_retries + 1,
                            request_id
                        );
                    }

                    // Execute subtask using configured execution mode
                    let result = if use_subagent_execution {
                        // Sub-Agent execution mode
                        execute_subtask_via_subagent(
                            &state,
                            &chat_server,
                            &headers,
                            subtask,
                            &subtask_results,
                            &available_tools,
                            Some(&skills_summaries),
                            attempt_timeout,
                            &cancel_token,
                            request_id,
                            &mut subtask_trace,
                            &model_name,
                            emitter.as_ref(),
                            subagent_config.as_ref(),
                            Some(time_budget.pause_tracker()),
                            &file_attachments,
                        )
                        .await
                    } else {
                        // Direct execution mode (React loop)
                        execute_subtask_with_react(
                            &state,
                            &chat_server,
                            &headers,
                            subtask,
                            &subtask_results,
                            &available_tools,
                            Some(&skills_summaries),
                            conv_id.as_deref(),
                            &user_id,
                            attempt_timeout,
                            subtask_react_max_iterations,
                            max_tools_per_iteration,
                            tool_call_max_retries,
                            tool_call_retry_delay_ms,
                            &cancel_token,
                            request_id,
                            &mut subtask_trace,
                            &model_name,
                            emitter.as_ref(),
                            Some(time_budget.pause_tracker()),
                            &file_attachments,
                        )
                        .await
                    };

                    match result {
                        Ok(result_text) => {
                            // Perform reflection on the result (if enabled)
                            let should_retry = if let Some(ref engine) = reflection_engine {
                                // Emit reflecting status event
                                emitter
                                    .emit_status(
                                        ExecutionPhase::Reflecting,
                                        &format!("Reflecting on subtask {} result", subtask.id),
                                        Some(subtask.id),
                                        Some(subtask_idx + 1),
                                        Some(total_subtasks),
                                    )
                                    .await;
                                // Build reflection context
                                let deps_results: Vec<String> = subtask
                                    .dependencies
                                    .iter()
                                    .filter_map(|dep_id| {
                                        subtask_results
                                            .iter()
                                            .find(|(id, _)| *id == *dep_id)
                                            .map(|(_, r)| r.clone())
                                    })
                                    .collect();

                                let tool_calls: Vec<String> = subtask_trace
                                    .react_iterations
                                    .iter()
                                    .flat_map(|it| {
                                        it.tool_calls.iter().map(|tc| tc.tool_name.clone())
                                    })
                                    .collect();

                                let context = ReflectionContext::new(&subtask.description)
                                    .with_dependencies(deps_results)
                                    .with_iterations(subtask_trace.react_iterations.len() as u32)
                                    .with_tool_calls(tool_calls)
                                    .with_time_taken(
                                        subtask_start_time.elapsed().as_millis() as u64
                                    );

                                // Check cache first (if enabled)
                                let cached_result = if let Some(ref cache) = reflection_cache {
                                    cache.get(&subtask.description, &result_text)
                                } else {
                                    None
                                };

                                if let Some(cached) = cached_result {
                                    dual_debug!(
                                        "🔍 Using cached reflection for subtask {} - request_id: {}",
                                        subtask.id,
                                        request_id
                                    );

                                    // Record cached reflection result to subtask trace
                                    subtask_trace.set_reflection(
                                        SubtaskReflectionSummary::from_result(
                                            &cached, true, // from_cache = true
                                        ),
                                    );

                                    // Use cached reflection result
                                    match cached.recommended_action {
                                        RecommendedAction::Accept
                                        | RecommendedAction::AcceptWithFix(_) => false,
                                        RecommendedAction::Retry
                                        | RecommendedAction::RetryWithStrategy(_) => {
                                            attempt < subtask_max_retries
                                        }
                                        _ => false,
                                    }
                                } else {
                                    // Perform fresh reflection
                                    match engine
                                        .reflect_on_subtask(
                                            subtask,
                                            &result_text,
                                            &subtask_trace,
                                            &context,
                                        )
                                        .await
                                    {
                                        Ok(reflection) => {
                                            dual_info!(
                                                "🔍 Reflection for subtask {}: {} - request_id: {}",
                                                subtask.id,
                                                reflection.summary(),
                                                request_id
                                            );

                                            // Record fresh reflection result to subtask trace
                                            subtask_trace.set_reflection(
                                                SubtaskReflectionSummary::from_result(
                                                    &reflection,
                                                    false, // from_cache = false
                                                ),
                                            );

                                            // Store in cache
                                            if let Some(ref cache) = reflection_cache {
                                                cache.put(
                                                    &subtask.description,
                                                    &result_text,
                                                    reflection.clone(),
                                                );
                                            }

                                            // Update adaptive strategy
                                            if let Some(ref strategy) = adaptive_strategy {
                                                strategy.record_outcome(
                                                    &subtask.description,
                                                    reflection.passed,
                                                    reflection.reflection_rounds,
                                                    reflection.confidence,
                                                );
                                            }

                                            // Determine if retry is needed based on reflection
                                            match &reflection.recommended_action {
                                                RecommendedAction::Accept
                                                | RecommendedAction::AcceptWithFix(_) => false,
                                                RecommendedAction::Retry
                                                | RecommendedAction::RetryWithStrategy(_) => {
                                                    if !reflection.passed
                                                        && attempt < subtask_max_retries
                                                    {
                                                        dual_warn!(
                                                            "🔄 Reflection suggests retry for subtask {} (confidence: {:.2}) - request_id: {}",
                                                            subtask.id,
                                                            reflection.confidence,
                                                            request_id
                                                        );
                                                        subtask_trace.record_retry(format!(
                                                            "Reflection: {}",
                                                            reflection
                                                                .issues
                                                                .first()
                                                                .map(|i| i.description.as_str())
                                                                .unwrap_or("Low confidence")
                                                        ));
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                }
                                                RecommendedAction::Replan(replan_request) => {
                                                    // R5.2: Handle reflection-suggested replan
                                                    // Note: Actual replanning is deferred to after subtask borrow ends
                                                    dual_info!(
                                                        "🔄 Reflection suggests replan for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        replan_request.reason,
                                                        request_id
                                                    );
                                                    // For now, just log and accept the result
                                                    // Full replanning integration is handled via failure path
                                                    false
                                                }
                                                RecommendedAction::RequestClarification(msg) => {
                                                    dual_warn!(
                                                        "❓ Reflection requests clarification for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        msg,
                                                        request_id
                                                    );
                                                    false
                                                }
                                                RecommendedAction::Abort(reason) => {
                                                    dual_warn!(
                                                        "⛔ Reflection suggests abort for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        reason,
                                                        request_id
                                                    );
                                                    false
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            // Reflection failed, log and continue without retry
                                            dual_warn!(
                                                "⚠️ Reflection failed for subtask {}: {} - request_id: {}",
                                                subtask.id,
                                                e,
                                                request_id
                                            );
                                            false
                                        }
                                    }
                                }
                            } else {
                                false
                            };

                            if should_retry {
                                // Continue to next retry attempt
                                last_error = Some(ServerError::Operation(
                                    "Reflection suggested retry".to_string(),
                                ));
                                continue;
                            }

                            dual_info!(
                                "✅ Subtask {} completed (attempt {}) - request_id: {}",
                                subtask.id,
                                attempt + 1,
                                request_id
                            );

                            subtask.complete(result_text.clone());
                            completed_subtasks.insert(subtask.id);
                            subtask_results.push((subtask.id, result_text.clone()));
                            subtask_trace.complete(result_text);
                            last_error = None;
                            break;
                        }
                        Err(e) => {
                            // Check if this error is retryable
                            if is_retryable_error(&e) && attempt < subtask_max_retries {
                                dual_warn!(
                                    "⚠️ Subtask {} failed with retryable error: {} - request_id: {}",
                                    subtask.id,
                                    e,
                                    request_id
                                );
                                subtask_trace.record_retry(e.to_string());
                                last_error = Some(e);
                                // Continue to next retry attempt
                            } else {
                                // Non-retryable error or max retries exceeded
                                dual_warn!(
                                    "❌ Subtask {} failed: {} - request_id: {}",
                                    subtask.id,
                                    e,
                                    request_id
                                );
                                last_error = Some(e);
                                break;
                            }
                        }
                    }
                }

                // Handle final result after retry loop
                if let Some(error) = last_error {
                    // Check if this is a user interruption - abort the entire plan immediately
                    if matches!(error, ServerError::UserInterrupted(_)) {
                        dual_warn!(
                            "🛑 Plan execution aborted due to user interruption: {} - request_id: {}",
                            error,
                            request_id
                        );
                        subtask.fail(error.to_string());
                        subtask_trace.fail(error.to_string());
                        trace.add_subtask_trace(subtask_trace);
                        return Err(error);
                    }

                    // Check if we exhausted all retries
                    if subtask_trace.retry_count >= subtask_max_retries && subtask_max_retries > 0 {
                        let retry_exhausted_error = ServerError::SubtaskRetryExhausted {
                            subtask_id: subtask.id,
                            attempts: subtask_trace.retry_count + 1,
                            message: error.to_string(),
                        };
                        subtask.fail(retry_exhausted_error.to_string());
                        subtask_trace.fail(retry_exhausted_error.to_string());
                    } else {
                        subtask.fail(error.to_string());
                        subtask_trace.fail(error.to_string());
                    }

                    // Continue execution (don't fail the entire plan)
                }

                dual_info!("Subtask trace: {}", subtask_trace.summary());
                trace.add_subtask_trace(subtask_trace);
                pending_count = pending_count.saturating_sub(1);

                // R5.2: Check if replanning should be triggered after failure
                // This is done after subtask borrow ends to avoid conflicts
                if let Some(ref replanner) = dynamic_replanner {
                    let replan_config = replanner.config().clone();

                    if let Some(trigger) =
                        ReplanTrigger::should_replan(&trace, &replan_config, &dependency_graph)
                    {
                        if replan_count < MAX_REPLAN_ATTEMPTS {
                            dual_info!(
                                "🔄 Replan triggered: {} - request_id: {}",
                                trigger.description(),
                                request_id
                            );

                            // Capture plan info
                            let original_goal = plan.original_goal.clone();
                            let pending = extract_pending_subtasks(&plan, &completed_subtasks);

                            // Capture trigger description before move
                            let trigger_desc = format!("{:?}", trigger);

                            // Execute replanning
                            match execute_replan(
                                replanner,
                                &original_goal,
                                pending,
                                &subtask_results,
                                &trace,
                                trigger,
                                Some(time_budget.remaining()),
                                request_id,
                            )
                            .await
                            {
                                Ok(replan_result) => {
                                    // Record the replan event to trace
                                    trace.add_replan_event(ReplanEvent {
                                        timestamp: chrono::Utc::now(),
                                        trigger: trigger_desc,
                                        preserved_count: replan_result.preserved_subtasks.len(),
                                        added_count: replan_result.added_subtasks.len(),
                                        removed_count: replan_result.removed_subtasks.len(),
                                    });

                                    // Apply the new plan
                                    apply_new_plan(&mut plan, replan_result, request_id);
                                    replan_count += 1;

                                    // Reset state for new plan execution
                                    completed_subtasks.clear();
                                    for (id, _) in &subtask_results {
                                        completed_subtasks.insert(*id);
                                    }
                                    pending_count = plan.execution_order.len();

                                    dual_info!(
                                        "🔄 Restarting execution with new plan (attempt {}/{}) - request_id: {}",
                                        replan_count,
                                        MAX_REPLAN_ATTEMPTS,
                                        request_id
                                    );
                                    continue 'execution;
                                }
                                Err(e) => {
                                    dual_warn!(
                                        "⚠️ Replanning failed: {} - continuing with current plan - request_id: {}",
                                        e,
                                        request_id
                                    );
                                }
                            }
                        } else {
                            dual_warn!(
                                "⚠️ Max replan attempts ({}) reached - request_id: {}",
                                MAX_REPLAN_ATTEMPTS,
                                request_id
                            );
                        }
                    }
                }
            }

            // All subtasks in current plan completed, exit the execution loop
            break 'execution;
        }
    } // End of else block (sequential execution)

    // ========================================================================
    // Phase 3: Result Aggregation
    // ========================================================================

    dual_info!("📊 Aggregating results - request_id: {}", request_id);

    // Compute reflection summary before finalizing trace
    trace.compute_reflection_summary();

    let final_response = generate_final_response(
        &state,
        &chat_server,
        &headers,
        &request,
        &plan,
        &subtask_results,
        request_id,
    )
    .await?;

    let final_content = final_response.choices[0]
        .message
        .content
        .clone()
        .unwrap_or_default();

    dual_info!("✅ Plan execution completed - request_id: {}", request_id);

    // Store assistant message to memory
    if let (Some(memory), Some(conv_id)) = (&state.memory, &conv_id)
        && let Err(e) = memory
            .add_assistant_message(conv_id, &final_content, vec![], is_privacy)
            .await
    {
        dual_error!(
            "Failed to add assistant message to memory: {} - request_id: {}",
            e,
            request_id
        );
    }

    // Write assistant message to session history
    write_assistant_to_session(&state, &session_id, &request, &final_content, is_privacy).await;

    // Trigger async memory recording for plan result (fire-and-forget)
    if !is_privacy && state.has_memory_writer() {
        let state_clone = Arc::clone(&state);
        let chat_url = format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
        let api_key = chat_server.api_key.clone();
        let model = model_name.clone();
        let user_msg = user_request.clone();
        let plan_result = final_content.clone();
        tokio::spawn(async move {
            trigger_plan_memory(
                &state_clone,
                &chat_url,
                api_key.as_deref(),
                &model,
                &user_msg,
                &plan_result,
            )
            .await;
        });
    }

    // Finalize trace
    trace.finalize(TraceStatus::Success);
    dual_info!("Plan trace: {}", trace.summary());
    dual_debug!(
        "Plan trace details:\n{}",
        serde_json::to_string_pretty(&trace).unwrap_or_default()
    );

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

    // Calculate execution statistics for finish event
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

    // Emit finish event
    let execution_summary = ExecutionSummary {
        subtask_count: trace.subtask_traces.len(),
        completed_count,
        failed_count,
        tool_call_count,
        duration_ms: trace.total_duration.as_millis() as u64,
    };
    emitter
        .emit_finish(&trace.total_tokens, "stop", None, Some(execution_summary))
        .await;

    // Build response
    let response = build_response(
        final_response,
        &final_content,
        stream,
        &enhanced_stream_config,
        event_receiver,
        request_id,
    );

    // Cleanup any pending HITL requests for this conversation (batch mode)
    if let Some(ref cid) = conv_id
        && let Some(hitl_manager) = hitl::global()
    {
        let cancelled = hitl_manager
            .cancel_by_conversation(cid, "Batch request completed")
            .await;
        if cancelled > 0 {
            dual_info!(
                "Cleaned up {} pending HITL requests for conversation {} - request_id: {}",
                cancelled,
                cid,
                request_id
            );
        }
    }

    response
}

// ============================================================================
// Realtime Streaming Mode
// ============================================================================

/// Realtime streaming handler for Plan mode chat.
///
/// This function immediately returns an SSE stream and executes the plan in a
/// background task. Events are sent to the client in real-time as they occur,
/// rather than being batched after execution completes.
#[allow(clippy::too_many_arguments)]
async fn chat_realtime_stream(
    state: Arc<AppState>,
    cancel_token: CancellationToken,
    headers: HeaderMap,
    request: ChatCompletionRequest,
    conv_id: Option<String>,
    session_id: Option<String>,
    request_id: String,
    enhanced_stream_config: EnhancedStreamConfig,
    time_budget: TimeBudget,
    reflection_engine: Option<ReflectionEngine>,
    reflection_cache: Option<ReflectionCache>,
    adaptive_strategy: Option<AdaptiveStrategy>,
    dynamic_replanner: Option<DynamicReplanner>,
    reflection_config: crate::reflection::ReflectionConfig,
    file_attachments: Vec<FileAttachmentInfo>,
) -> ServerResult<axum::response::Response> {
    // Create channel for realtime event streaming
    // Events will be sent through this channel as they occur
    let (event_sender, event_receiver) = mpsc::channel::<String>(256);

    // Build and return the SSE response immediately
    // The actual execution happens in a background task
    let response = build_realtime_streaming_response(event_receiver, &request_id)?;

    // Clone values needed for the background task
    let request_id_clone = request_id.clone();

    // Clone conv_id for cleanup
    let conv_id_for_cleanup = conv_id.clone();

    // Spawn background task to execute the plan
    tokio::spawn(async move {
        let result = execute_chat_plan_realtime(
            state,
            cancel_token,
            headers,
            request,
            conv_id,
            session_id,
            request_id_clone.clone(),
            enhanced_stream_config,
            time_budget,
            reflection_engine,
            reflection_cache,
            adaptive_strategy,
            dynamic_replanner,
            reflection_config,
            event_sender,
            file_attachments,
        )
        .await;

        if let Err(e) = result {
            // Use WARN for user interruption, ERROR for other failures
            if matches!(e, ServerError::UserInterrupted(_)) {
                dual_warn!(
                    "Realtime chat execution interrupted by user: {} - request_id: {}",
                    e,
                    request_id_clone
                );
            } else {
                dual_error!(
                    "Realtime chat execution failed: {} - request_id: {}",
                    e,
                    request_id_clone
                );
            }
        }

        // Cleanup any pending HITL requests for this conversation
        // This handles cases where the SSE connection closes or the task ends
        if let Some(ref cid) = conv_id_for_cleanup
            && let Some(hitl_manager) = hitl::global()
        {
            let cancelled = hitl_manager
                .cancel_by_conversation(cid, "SSE connection closed")
                .await;
            if cancelled > 0 {
                dual_info!(
                    "Cleaned up {} pending HITL requests for conversation {} - request_id: {}",
                    cancelled,
                    cid,
                    request_id_clone
                );
            }
        }
    });

    Ok(response)
}

/// Executes the chat plan in realtime streaming mode.
///
/// This function contains the main execution logic, sending events through the
/// provided channel as they occur. It is designed to run as a background task.
#[allow(clippy::too_many_arguments)]
async fn execute_chat_plan_realtime(
    state: Arc<AppState>,
    cancel_token: CancellationToken,
    headers: HeaderMap,
    mut request: ChatCompletionRequest,
    conv_id: Option<String>,
    session_id: Option<String>,
    request_id: String,
    enhanced_stream_config: EnhancedStreamConfig,
    time_budget: TimeBudget,
    reflection_engine: Option<ReflectionEngine>,
    reflection_cache: Option<ReflectionCache>,
    adaptive_strategy: Option<AdaptiveStrategy>,
    dynamic_replanner: Option<DynamicReplanner>,
    _reflection_config: crate::reflection::ReflectionConfig,
    event_sender: mpsc::Sender<String>,
    file_attachments: Vec<FileAttachmentInfo>,
) -> ServerResult<()> {
    use super::{
        emitter::SseEventEmitter,
        events::{PlanEvent, PlanSubtask, TextEvent, format_sse_event},
    };

    // Create emitter that writes directly to the event sender
    // Use Arc to allow sharing across parallel subtask executions
    let emitter: Arc<dyn EventEmitter> = Arc::new(SseEventEmitter::new(
        event_sender.clone(),
        enhanced_stream_config.clone(),
    ));

    // Spawn HitlNotifier if HITL is enabled to bridge HITL events to SSE
    if let Some(hitl_manager) = crate::services::hitl::global() {
        crate::services::hitl::HitlNotifier::spawn(
            std::sync::Arc::clone(hitl_manager),
            emitter.clone(),
        );
        tracing::debug!("HitlNotifier spawned for realtime SSE connection");
    }

    // Get target server (route based on X-Privacy-Mode header)
    let server_kind = resolve_chat_server_kind(&headers);
    let chat_server = get_chat_server(&state, &request_id, server_kind).await?;
    let is_privacy = server_kind == ServerKind::privacy_chat;

    // Extract user message for planning (use passed file_attachments, re-extract text only)
    let user_message = extract_user_message(&request);

    // Extract system message for memory storage
    let system_message = extract_system_message(&request);

    // file_attachments is available from parent scope

    // Store the latest user message to memory
    if let Some(memory) = &state.memory
        && let Some(conv_id) = &conv_id
        && let Some(user_msg) = &user_message
    {
        // Handle system message storage
        if let Some(sys_msg) = &system_message
            && let Ok(updated) = memory.set_system_message(conv_id, sys_msg).await
            && updated
        {
            dual_debug!(
                "System message updated for conversation {} - request_id: {}",
                conv_id,
                request_id
            );
        }

        // Store user message
        if let Err(e) = memory
            .add_user_message(conv_id, user_msg.clone(), is_privacy)
            .await
        {
            dual_error!(
                "Failed to add user message to memory: {} - request_id: {}",
                e,
                request_id
            );
        }
    }

    // NOTE: User message is already written to session in the parent `chat()` function
    // before branching into realtime mode. No duplicate write needed here.

    // Get plan mode configuration
    let (
        max_plan_subtasks,
        subtask_max_retries,
        subtask_react_max_iterations,
        subtask_react_timeout_secs,
        max_tools_per_iteration,
        tool_call_max_retries,
        tool_call_retry_delay_ms,
        subagent_config,
    ) = {
        let config = state.config.read().await;
        (
            config.server.max_plan_subtasks,
            config.server.subtask_max_retries,
            config.server.subtask_react_max_iterations,
            config.server.subtask_react_timeout_secs,
            config.server.max_tools_per_iteration,
            config.server.tool_call_max_retries,
            config.server.tool_call_retry_delay_ms,
            config.subagent.clone(),
        )
    };

    // Determine execution mode for subtasks
    let use_subagent_execution = subagent_config
        .as_ref()
        .map(|c| c.is_subagent_mode())
        .unwrap_or(false);

    // Log the execution mode for debugging
    let execution_mode_str = subagent_config
        .as_ref()
        .map(|c| c.execution_mode.as_str())
        .unwrap_or("direct (default)");
    dual_info!(
        "🎯 Execution mode: {} (use_subagent={}) - request_id: {}",
        execution_mode_str,
        use_subagent_execution,
        request_id
    );

    // Disable streaming for internal LLM calls
    request.stream = Some(false);

    // ========================================================================
    // Phase 1: Task Planning
    // ========================================================================

    dual_info!("📋 Starting task planning - request_id: {}", request_id);

    // Emit planning status event
    emitter
        .emit_status(
            ExecutionPhase::Planning,
            "Analyzing user request and generating task plan...",
            None,
            None,
            None,
        )
        .await;

    let user_request = user_message.clone().unwrap_or_default();

    // Get available tools from MCP services (with full parameter schemas)
    let available_tools = get_available_tools(&state).await;

    // Get available Skills summaries from global registry
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
        Err(_) => {
            // Skills registry not initialized, continue without skills
            dual_debug!("Skills registry not available - request_id: {}", request_id);
            vec![]
        }
    };

    // Get model name from request, fallback to "default" if not specified
    let model_name = request
        .model
        .clone()
        .unwrap_or_else(|| "default".to_string());

    // Read recent conversation history from Session JSONL for multi-turn context
    let chat_history: Vec<PlannerMessage> = if let Some(ref sid) = session_id
        && let Some(ref writer) = state.session_writer
    {
        let uid = request.user.as_deref().unwrap_or("anonymous");
        let reader = crate::session::reader::SessionReader::new(writer.base_dir());
        match reader.read_session(uid, sid).await {
            Ok(records) => {
                let all_messages: Vec<PlannerMessage> = records
                    .into_iter()
                    .filter_map(|r| match r {
                        crate::session::types::SessionRecord::Message {
                            role,
                            content,
                            privacy_mode,
                            ..
                        } => {
                            if privacy_mode {
                                return None;
                            }
                            match role.as_str() {
                                "user" => Some(PlannerMessage::user(content)),
                                "assistant" => Some(PlannerMessage::assistant(content)),
                                _ => None,
                            }
                        }
                        _ => None,
                    })
                    .collect();
                // Skip the last message (current user message already written to JSONL)
                // and take up to 10 recent messages
                let len = all_messages.len();
                if len > 1 {
                    let start = (len - 1).saturating_sub(10);
                    all_messages[start..len - 1].to_vec()
                } else {
                    vec![]
                }
            }
            Err(_) => vec![],
        }
    } else {
        vec![]
    };

    // Create task planner
    let mut planner = TaskPlanner::with_chat_llm(
        format!("{}/chat/completions", chat_server.url.trim_end_matches('/')),
        chat_server.api_key.clone(),
        model_name.clone(),
        max_plan_subtasks,
    )
    .with_tools(available_tools.clone())
    .with_skills(skills_summaries.clone())
    .with_chat_history(chat_history);

    // Add memory guidance and context when memory tools are available
    if state.has_memory_writer() {
        let mut memory_rules = vec![
            "You have access to a personal knowledge base with memory tools (lantai_write_memory, lantai_search, etc.). Use TaskPlan with memory tools ONLY when the user explicitly asks to save, remember, recall, or look up PERSONAL information (their preferences, decisions, notes, past instructions). For general knowledge questions (facts, geography, science, history, common knowledge, etc.), use DirectAnswer — do NOT route them through memory tools.".to_string(),
        ];

        // Inject recent memory context into planner so DirectAnswer can leverage past knowledge
        let lantai_cfg = state.config.read().await.lantai.clone();
        let context_injection_enabled = lantai_cfg
            .as_ref()
            .map(|l| l.auto_memory.context_injection)
            .unwrap_or(true);
        if context_injection_enabled && let Some(writer) = state.memory_writer() {
            let max_chars = lantai_cfg
                .as_ref()
                .map(|l| l.auto_memory.max_context_chars)
                .unwrap_or(2000);
            let memory_context = load_memory_context(writer, max_chars).await;
            if !memory_context.is_empty() {
                memory_rules.push(format!(
                    "\n## Recent Memory Context\n{memory_context}\n\nUse this context to inform your responses when relevant."
                ));
            }
        }

        planner = planner.with_extra_rules(memory_rules);
    }

    // Generate task plan or direct answer
    let planner_output = match planner.plan(&user_request).await {
        Ok(output) => output,
        Err(e) => {
            dual_error!(
                "Failed to generate task plan: {} - request_id: {}",
                e,
                request_id
            );
            // Send error event before returning
            let error_event = format_sse_event(
                "error",
                &serde_json::json!({
                    "message": format!("Failed to generate task plan: {}", e)
                }),
            );
            let _ = event_sender.send(error_event).await;
            let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
            return Err(e);
        }
    };

    // Handle direct answer case - return immediately without task execution
    let mut plan = match planner_output {
        PlannerOutput::DirectAnswer(answer) => {
            dual_info!("📝 Direct answer mode - request_id: {}", request_id);

            // Store assistant response to memory if available
            if let Some(memory) = &state.memory
                && let Some(conv_id) = &conv_id
                && let Err(e) = memory
                    .add_assistant_message(conv_id, &answer.answer, vec![], is_privacy)
                    .await
            {
                dual_error!(
                    "Failed to add assistant message to memory: {} - request_id: {}",
                    e,
                    request_id
                );
            }

            // Write assistant message to session history
            write_assistant_to_session(&state, &session_id, &request, &answer.answer, is_privacy)
                .await;

            // Trigger async memory recording for direct answer (fire-and-forget)
            if !is_privacy && state.has_memory_writer() {
                let state_clone = Arc::clone(&state);
                let chat_url =
                    format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
                let api_key = chat_server.api_key.clone();
                let model = model_name.clone();
                let user_msg = user_request.clone();
                let assistant_msg = answer.answer.clone();
                tokio::spawn(async move {
                    trigger_direct_answer_memory(
                        &state_clone,
                        &chat_url,
                        api_key.as_deref(),
                        &model,
                        &user_msg,
                        &assistant_msg,
                    )
                    .await;
                });
            }

            // Send text events for direct answer
            let text_chunks = gen_chunks_with_formatting(&answer.answer, 10);
            for chunk in text_chunks {
                let event = format_sse_event("text", &TextEvent { content: chunk });
                if event_sender.send(event).await.is_err() {
                    dual_info!("Client disconnected - request_id: {}", request_id);
                    return Ok(());
                }
            }

            // Send done marker
            let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
            return Ok(());
        }
        PlannerOutput::TaskPlan(raw_plan) => {
            // Convert raw plan to validated TaskPlan
            match TaskPlan::from_raw(raw_plan) {
                Ok(plan) => plan,
                Err(e) => {
                    let error_event = format_sse_event(
                        "error",
                        &serde_json::json!({
                            "message": format!("Invalid task plan: {}", e)
                        }),
                    );
                    let _ = event_sender.send(error_event).await;
                    let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
                    return Err(e);
                }
            }
        }
    };

    dual_info!(
        "📋 Task plan generated: {} subtasks - request_id: {}",
        plan.len(),
        request_id
    );

    // Log the plan
    for (i, subtask) in plan.subtasks.iter().enumerate() {
        let skill_info = subtask
            .recommended_skill
            .as_ref()
            .map(|s| format!(", skill: {}", s))
            .unwrap_or_default();
        dual_debug!(
            "  Subtask {}: {} (deps: {:?}, tools: {:?}{})",
            subtask.id,
            subtask.description,
            subtask.dependencies,
            subtask.required_tools,
            skill_info
        );
        if i < plan.execution_order.len() {
            dual_debug!("  Execution order[{}]: {}", i, plan.execution_order[i]);
        }
    }

    // Send plan event to frontend with subtask list
    let plan_subtasks: Vec<PlanSubtask> = plan
        .subtasks
        .iter()
        .map(|s| {
            let status_str = match &s.status {
                super::planner::SubTaskStatus::Pending => "pending",
                super::planner::SubTaskStatus::InProgress => "in_progress",
                super::planner::SubTaskStatus::Completed => "completed",
                super::planner::SubTaskStatus::Failed(_) => "failed",
                super::planner::SubTaskStatus::Skipped => "skipped",
            };
            PlanSubtask::new(s.id, &s.description, status_str)
        })
        .collect();
    let plan_event = PlanEvent::new(&plan.original_goal, plan_subtasks);
    let plan_event_str = format_sse_event("plan", &plan_event);
    if event_sender.send(plan_event_str).await.is_err() {
        dual_info!(
            "Client disconnected while sending plan event - request_id: {}",
            request_id
        );
        return Ok(());
    }

    // Initialize execution trace
    let mut trace = PlanTrace::new(
        request_id.to_string(),
        plan.original_goal.clone(),
        plan.execution_order.clone(),
    );
    trace.start();

    // ========================================================================
    // Phase 2: Task Execution (React Loop per Subtask)
    // ========================================================================

    dual_info!("🚀 Starting task execution - request_id: {}", request_id);

    // Extract user_id from headers for HITL context
    let user_id = headers
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    // Emit executing status event with subtask count
    let total_subtasks = plan.execution_order.len();
    emitter
        .emit_status(
            ExecutionPhase::Executing,
            &format!("Executing {} subtasks...", total_subtasks),
            None,
            Some(0),
            Some(total_subtasks),
        )
        .await;

    let mut completed_subtasks: HashSet<usize> = HashSet::new();
    let mut subtask_results: Vec<(usize, String)> = Vec::new();

    // Build dependency graph for critical subtask detection (R5.2)
    #[allow(unused_variables)]
    let dependency_graph = DependencyGraph::from_subtask_dependencies(
        &plan.subtasks.iter().map(|s| s.id).collect::<Vec<_>>(),
        &plan
            .subtasks
            .iter()
            .map(|s| s.dependencies.clone())
            .collect::<Vec<_>>(),
    );

    // Calculate pending subtask count for time allocation
    let mut pending_count = plan.execution_order.len();

    // Replan tracking (for R5.2 dynamic replanner integration)
    #[allow(unused_variables, unused_mut)]
    let mut replan_count = 0u32;
    #[allow(dead_code)]
    const MAX_REPLAN_ATTEMPTS: u32 = 3;

    // Check if parallel execution should be used
    let use_parallel_execution =
        should_use_parallel_execution(subagent_config.as_ref(), plan.execution_order.len());

    if use_parallel_execution {
        dual_info!(
            "🚀 Using parallel execution for {} subtasks (stream) - request_id: {}",
            plan.execution_order.len(),
            request_id
        );

        // Create parallel execution context
        let parallel_ctx = ParallelExecutionContext {
            state: state.clone(),
            chat_server: chat_server.clone(),
            headers: headers.clone(),
            available_tools: available_tools.clone(),
            skills_summaries: skills_summaries.clone(),
            cancel_token: cancel_token.clone(),
            request_id: request_id.to_string(),
            model_name: model_name.clone(),
            subagent_config: subagent_config.clone(),
            rate_limiter: None, // Rate limiter can be added if needed
            emitter: emitter.clone(),
            file_attachments: file_attachments.clone(),
        };

        // Execute subtasks in parallel
        if let Err(e) = execute_subtasks_parallel(
            &parallel_ctx,
            &mut plan.subtasks,
            &mut subtask_results,
            &mut completed_subtasks,
            &time_budget,
            emitter.as_ref(),
            &mut trace,
            total_subtasks,
            time_budget.pause_tracker(),
        )
        .await
        {
            // Check if this is a user interruption - send proper SSE events before returning
            if matches!(e, ServerError::UserInterrupted(_)) {
                dual_warn!(
                    "🛑 Plan execution aborted due to user interruption (parallel mode): {} - request_id: {}",
                    e,
                    request_id
                );
                trace.finalize(TraceStatus::Error(e.to_string()));

                // Send interruption event to frontend
                let error_event = format_sse_event(
                    "error",
                    &serde_json::json!({
                        "message": e.to_string(),
                        "type": "user_interrupted"
                    }),
                );
                let _ = event_sender.send(error_event).await;
                let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
                return Err(e);
            }
            // For other errors, propagate normally
            return Err(e);
        }
    } else {
        // Sequential execution (existing code)
        dual_info!(
            "📋 Using sequential execution for {} subtasks (stream) - request_id: {}",
            plan.execution_order.len(),
            request_id
        );

        'execution: loop {
            let execution_order = plan.execution_order.clone();

            for &subtask_idx in &execution_order {
                // Check if client disconnected
                if event_sender.is_closed() {
                    dual_info!(
                        "Client disconnected, stopping execution - request_id: {}",
                        request_id
                    );
                    return Ok(());
                }

                // Check time budget
                if time_budget.is_exhausted() {
                    dual_warn!(
                        "Plan time budget exhausted after {} seconds - request_id: {}",
                        time_budget.elapsed().as_secs(),
                        request_id
                    );
                    trace.finalize(TraceStatus::Timeout);
                    dual_info!("Plan trace: {}", trace.summary());

                    // Send timeout error event
                    let error_event = format_sse_event(
                        "error",
                        &serde_json::json!({
                            "message": format!("Plan execution timed out after {} seconds", time_budget.elapsed().as_secs())
                        }),
                    );
                    let _ = event_sender.send(error_event).await;
                    let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
                    return Err(ServerError::TimeBudgetExhausted {
                        elapsed_secs: time_budget.elapsed().as_secs(),
                    });
                }

                // Check cancellation
                if cancel_token.is_cancelled() {
                    let warn_msg = "Request was cancelled by client";
                    dual_warn!("{} - request_id: {}", warn_msg, request_id);
                    trace.finalize(TraceStatus::Error(warn_msg.to_string()));

                    let error_event = format_sse_event(
                        "error",
                        &serde_json::json!({
                            "message": warn_msg
                        }),
                    );
                    let _ = event_sender.send(error_event).await;
                    let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
                    return Err(ServerError::Operation(warn_msg.to_string()));
                }

                let subtask = match plan.subtasks.get_mut(subtask_idx) {
                    Some(s) => s,
                    None => continue,
                };

                // Initialize subtask trace
                let mut subtask_trace = SubtaskTrace::new(subtask.id, subtask.description.clone());

                dual_info!(
                    "▶️ Executing subtask {}: {} - request_id: {}",
                    subtask.id,
                    subtask.description,
                    request_id
                );

                // Emit status event for subtask start
                let subtask_current = completed_subtasks.len() + 1;
                emitter
                    .emit_status(
                        ExecutionPhase::Executing,
                        &format!("Executing subtask {}: {}", subtask.id, subtask.description),
                        Some(subtask.id),
                        Some(subtask_current),
                        Some(total_subtasks),
                    )
                    .await;

                // Check dependencies
                if !subtask.is_ready(&completed_subtasks) {
                    dual_warn!(
                        "Subtask {} has unmet dependencies, skipping - request_id: {}",
                        subtask.id,
                        request_id
                    );
                    subtask.skip();
                    subtask_trace.status = crate::chat::planner::SubTaskStatus::Skipped;
                    dual_info!("Subtask trace: {}", subtask_trace.summary());
                    trace.add_subtask_trace(subtask_trace);
                    pending_count = pending_count.saturating_sub(1);
                    continue;
                }

                // Start subtask execution
                subtask.start();
                subtask_trace.start();

                // Allocate time budget for this subtask (including potential retries)
                let subtask_time_budget = time_budget.allocate(pending_count);
                // Ensure we don't exceed the configured subtask timeout
                let effective_timeout =
                    subtask_time_budget.min(Duration::from_secs(subtask_react_timeout_secs));

                dual_debug!(
                    "Allocated {:?} for subtask {} (pending: {}, max_retries: {}) - request_id: {}",
                    effective_timeout,
                    subtask.id,
                    pending_count,
                    subtask_max_retries,
                    request_id
                );

                // Execute the subtask with retry loop
                let mut last_error: Option<ServerError> = None;
                let subtask_start_time = Instant::now();

                for attempt in 0..=subtask_max_retries {
                    // Check if client disconnected
                    if event_sender.is_closed() {
                        dual_info!(
                            "Client disconnected, stopping execution - request_id: {}",
                            request_id
                        );
                        return Ok(());
                    }

                    // Check if we've exceeded the total time budget for this subtask
                    let elapsed = subtask_start_time.elapsed();
                    if elapsed >= subtask_time_budget {
                        dual_warn!(
                            "Subtask {} time budget exhausted after {:?} - request_id: {}",
                            subtask.id,
                            elapsed,
                            request_id
                        );
                        last_error = Some(ServerError::SubtaskTimeout {
                            subtask_id: subtask.id,
                            timeout_secs: subtask_time_budget.as_secs(),
                        });
                        break;
                    }

                    // Calculate remaining time for this attempt
                    let remaining_time = subtask_time_budget.saturating_sub(elapsed);
                    let attempt_timeout = remaining_time.min(effective_timeout);

                    if attempt > 0 {
                        dual_info!(
                            "🔄 Retrying subtask {} (attempt {}/{}) - request_id: {}",
                            subtask.id,
                            attempt + 1,
                            subtask_max_retries + 1,
                            request_id
                        );
                    }

                    // Execute subtask using configured execution mode
                    let result = if use_subagent_execution {
                        // Sub-Agent execution mode
                        execute_subtask_via_subagent(
                            &state,
                            &chat_server,
                            &headers,
                            subtask,
                            &subtask_results,
                            &available_tools,
                            Some(&skills_summaries),
                            attempt_timeout,
                            &cancel_token,
                            &request_id,
                            &mut subtask_trace,
                            &model_name,
                            emitter.as_ref(),
                            subagent_config.as_ref(),
                            Some(time_budget.pause_tracker()),
                            &file_attachments,
                        )
                        .await
                    } else {
                        // Direct execution mode (React loop)
                        execute_subtask_with_react(
                            &state,
                            &chat_server,
                            &headers,
                            subtask,
                            &subtask_results,
                            &available_tools,
                            Some(&skills_summaries),
                            conv_id.as_deref(),
                            &user_id,
                            attempt_timeout,
                            subtask_react_max_iterations,
                            max_tools_per_iteration,
                            tool_call_max_retries,
                            tool_call_retry_delay_ms,
                            &cancel_token,
                            &request_id,
                            &mut subtask_trace,
                            &model_name,
                            emitter.as_ref(),
                            Some(time_budget.pause_tracker()),
                            &file_attachments,
                        )
                        .await
                    };

                    match result {
                        Ok(result_text) => {
                            // Perform reflection on the result (if enabled)
                            let should_retry = if let Some(ref engine) = reflection_engine {
                                // Emit reflecting status event
                                emitter
                                    .emit_status(
                                        ExecutionPhase::Reflecting,
                                        &format!("Reflecting on subtask {} result", subtask.id),
                                        Some(subtask.id),
                                        Some(subtask_idx + 1),
                                        Some(total_subtasks),
                                    )
                                    .await;
                                // Build reflection context
                                let deps_results: Vec<String> = subtask
                                    .dependencies
                                    .iter()
                                    .filter_map(|dep_id| {
                                        subtask_results
                                            .iter()
                                            .find(|(id, _)| *id == *dep_id)
                                            .map(|(_, r)| r.clone())
                                    })
                                    .collect();

                                let tool_calls: Vec<String> = subtask_trace
                                    .react_iterations
                                    .iter()
                                    .flat_map(|it| {
                                        it.tool_calls.iter().map(|tc| tc.tool_name.clone())
                                    })
                                    .collect();

                                let context = ReflectionContext::new(&subtask.description)
                                    .with_dependencies(deps_results)
                                    .with_iterations(subtask_trace.react_iterations.len() as u32)
                                    .with_tool_calls(tool_calls)
                                    .with_time_taken(
                                        subtask_start_time.elapsed().as_millis() as u64
                                    );

                                // Check cache first (if enabled)
                                let cached_result = if let Some(ref cache) = reflection_cache {
                                    cache.get(&subtask.description, &result_text)
                                } else {
                                    None
                                };

                                if let Some(cached) = cached_result {
                                    dual_debug!(
                                        "🔍 Using cached reflection for subtask {} - request_id: {}",
                                        subtask.id,
                                        request_id
                                    );

                                    // Record cached reflection result to subtask trace
                                    subtask_trace.set_reflection(
                                        SubtaskReflectionSummary::from_result(
                                            &cached, true, // from_cache = true
                                        ),
                                    );

                                    // Use cached reflection result
                                    match cached.recommended_action {
                                        RecommendedAction::Accept
                                        | RecommendedAction::AcceptWithFix(_) => false,
                                        RecommendedAction::Retry
                                        | RecommendedAction::RetryWithStrategy(_) => {
                                            attempt < subtask_max_retries
                                        }
                                        _ => false,
                                    }
                                } else {
                                    // Perform fresh reflection
                                    match engine
                                        .reflect_on_subtask(
                                            subtask,
                                            &result_text,
                                            &subtask_trace,
                                            &context,
                                        )
                                        .await
                                    {
                                        Ok(reflection) => {
                                            dual_info!(
                                                "🔍 Reflection for subtask {}: {} - request_id: {}",
                                                subtask.id,
                                                reflection.summary(),
                                                request_id
                                            );

                                            // Record fresh reflection result to subtask trace
                                            subtask_trace.set_reflection(
                                                SubtaskReflectionSummary::from_result(
                                                    &reflection,
                                                    false, // from_cache = false
                                                ),
                                            );

                                            // Store in cache
                                            if let Some(ref cache) = reflection_cache {
                                                cache.put(
                                                    &subtask.description,
                                                    &result_text,
                                                    reflection.clone(),
                                                );
                                            }

                                            // Update adaptive strategy
                                            if let Some(ref strategy) = adaptive_strategy {
                                                strategy.record_outcome(
                                                    &subtask.description,
                                                    reflection.passed,
                                                    reflection.reflection_rounds,
                                                    reflection.confidence,
                                                );
                                            }

                                            // Determine if retry is needed based on reflection
                                            match &reflection.recommended_action {
                                                RecommendedAction::Accept
                                                | RecommendedAction::AcceptWithFix(_) => false,
                                                RecommendedAction::Retry
                                                | RecommendedAction::RetryWithStrategy(_) => {
                                                    if !reflection.passed
                                                        && attempt < subtask_max_retries
                                                    {
                                                        dual_warn!(
                                                            "🔄 Reflection suggests retry for subtask {} (confidence: {:.2}) - request_id: {}",
                                                            subtask.id,
                                                            reflection.confidence,
                                                            request_id
                                                        );
                                                        subtask_trace.record_retry(format!(
                                                            "Reflection: {}",
                                                            reflection
                                                                .issues
                                                                .first()
                                                                .map(|i| i.description.as_str())
                                                                .unwrap_or("Low confidence")
                                                        ));
                                                        true
                                                    } else {
                                                        false
                                                    }
                                                }
                                                RecommendedAction::Replan(replan_request) => {
                                                    // R5.2: Handle reflection-suggested replan
                                                    dual_info!(
                                                        "🔄 Reflection suggests replan for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        replan_request.reason,
                                                        request_id
                                                    );
                                                    false
                                                }
                                                RecommendedAction::RequestClarification(msg) => {
                                                    dual_warn!(
                                                        "❓ Reflection requests clarification for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        msg,
                                                        request_id
                                                    );
                                                    false
                                                }
                                                RecommendedAction::Abort(reason) => {
                                                    dual_warn!(
                                                        "⛔ Reflection suggests abort for subtask {}: {} - request_id: {}",
                                                        subtask.id,
                                                        reason,
                                                        request_id
                                                    );
                                                    false
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            // Reflection failed, log and continue without retry
                                            dual_warn!(
                                                "⚠️ Reflection failed for subtask {}: {} - request_id: {}",
                                                subtask.id,
                                                e,
                                                request_id
                                            );
                                            false
                                        }
                                    }
                                }
                            } else {
                                false
                            };

                            if should_retry {
                                // Continue to next retry attempt
                                last_error = Some(ServerError::Operation(
                                    "Reflection suggested retry".to_string(),
                                ));
                                continue;
                            }

                            dual_info!(
                                "✅ Subtask {} completed (attempt {}) - request_id: {}",
                                subtask.id,
                                attempt + 1,
                                request_id
                            );

                            subtask.complete(result_text.clone());
                            completed_subtasks.insert(subtask.id);
                            subtask_results.push((subtask.id, result_text.clone()));
                            subtask_trace.complete(result_text);
                            last_error = None;
                            break;
                        }
                        Err(e) => {
                            // Check if this error is retryable
                            if is_retryable_error(&e) && attempt < subtask_max_retries {
                                dual_warn!(
                                    "⚠️ Subtask {} failed with retryable error: {} - request_id: {}",
                                    subtask.id,
                                    e,
                                    request_id
                                );
                                subtask_trace.record_retry(e.to_string());
                                last_error = Some(e);
                                // Continue to next retry attempt
                            } else {
                                // Non-retryable error or max retries exceeded
                                dual_warn!(
                                    "❌ Subtask {} failed: {} - request_id: {}",
                                    subtask.id,
                                    e,
                                    request_id
                                );
                                last_error = Some(e);
                                break;
                            }
                        }
                    }
                }

                // Handle final result after retry loop
                if let Some(error) = last_error {
                    // Check if this is a user interruption - abort the entire plan immediately
                    if matches!(error, ServerError::UserInterrupted(_)) {
                        dual_warn!(
                            "🛑 Plan execution aborted due to user interruption: {} - request_id: {}",
                            error,
                            request_id
                        );
                        subtask.fail(error.to_string());
                        subtask_trace.fail(error.to_string());
                        trace.add_subtask_trace(subtask_trace);
                        trace.finalize(TraceStatus::Error(error.to_string()));

                        // Send interruption event to frontend
                        let error_event = format_sse_event(
                            "error",
                            &serde_json::json!({
                                "message": error.to_string(),
                                "type": "user_interrupted"
                            }),
                        );
                        let _ = event_sender.send(error_event).await;
                        let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;
                        return Err(error);
                    }

                    // Check if we exhausted all retries
                    if subtask_trace.retry_count >= subtask_max_retries && subtask_max_retries > 0 {
                        let retry_exhausted_error = ServerError::SubtaskRetryExhausted {
                            subtask_id: subtask.id,
                            attempts: subtask_trace.retry_count + 1,
                            message: error.to_string(),
                        };
                        subtask.fail(retry_exhausted_error.to_string());
                        subtask_trace.fail(retry_exhausted_error.to_string());
                    } else {
                        subtask.fail(error.to_string());
                        subtask_trace.fail(error.to_string());
                    }

                    // Continue execution (don't fail the entire plan)
                }

                dual_info!("Subtask trace: {}", subtask_trace.summary());
                trace.add_subtask_trace(subtask_trace);
                pending_count = pending_count.saturating_sub(1);

                // R5.2: Check if replanning should be triggered after failure
                if let Some(ref replanner) = dynamic_replanner {
                    let replan_config = replanner.config().clone();

                    if let Some(trigger) =
                        ReplanTrigger::should_replan(&trace, &replan_config, &dependency_graph)
                    {
                        if replan_count < MAX_REPLAN_ATTEMPTS {
                            dual_info!(
                                "🔄 Replan triggered: {} - request_id: {}",
                                trigger.description(),
                                request_id
                            );

                            // Capture plan info
                            let original_goal = plan.original_goal.clone();
                            let pending = extract_pending_subtasks(&plan, &completed_subtasks);

                            // Capture trigger description before move
                            let trigger_desc = format!("{:?}", trigger);

                            // Execute replanning
                            match execute_replan(
                                replanner,
                                &original_goal,
                                pending,
                                &subtask_results,
                                &trace,
                                trigger,
                                Some(time_budget.remaining()),
                                &request_id,
                            )
                            .await
                            {
                                Ok(replan_result) => {
                                    // Record the replan event to trace
                                    trace.add_replan_event(ReplanEvent {
                                        timestamp: chrono::Utc::now(),
                                        trigger: trigger_desc,
                                        preserved_count: replan_result.preserved_subtasks.len(),
                                        added_count: replan_result.added_subtasks.len(),
                                        removed_count: replan_result.removed_subtasks.len(),
                                    });

                                    // Apply the new plan
                                    apply_new_plan(&mut plan, replan_result, &request_id);
                                    replan_count += 1;

                                    // Reset state for new plan execution
                                    completed_subtasks.clear();
                                    for (id, _) in &subtask_results {
                                        completed_subtasks.insert(*id);
                                    }
                                    pending_count = plan.execution_order.len();

                                    dual_info!(
                                        "🔄 Restarting execution with new plan (attempt {}/{}) - request_id: {}",
                                        replan_count,
                                        MAX_REPLAN_ATTEMPTS,
                                        request_id
                                    );
                                    continue 'execution;
                                }
                                Err(e) => {
                                    dual_warn!(
                                        "⚠️ Replanning failed: {} - continuing with current plan - request_id: {}",
                                        e,
                                        request_id
                                    );
                                }
                            }
                        } else {
                            dual_warn!(
                                "⚠️ Max replan attempts ({}) reached - request_id: {}",
                                MAX_REPLAN_ATTEMPTS,
                                request_id
                            );
                        }
                    }
                }
            }

            // All subtasks in current plan completed, exit the execution loop
            break 'execution;
        }
    } // End of else block (sequential execution)

    // ========================================================================
    // Phase 3: Result Aggregation
    // ========================================================================

    dual_info!("📊 Aggregating results - request_id: {}", request_id);

    // Compute reflection summary before finalizing trace
    trace.compute_reflection_summary();

    let final_response = generate_final_response(
        &state,
        &chat_server,
        &headers,
        &request,
        &plan,
        &subtask_results,
        &request_id,
    )
    .await?;

    let final_content = final_response.choices[0]
        .message
        .content
        .clone()
        .unwrap_or_default();

    dual_info!("✅ Plan execution completed - request_id: {}", request_id);

    // Store assistant message to memory
    if let (Some(memory), Some(conv_id)) = (&state.memory, &conv_id)
        && let Err(e) = memory
            .add_assistant_message(conv_id, &final_content, vec![], is_privacy)
            .await
    {
        dual_error!(
            "Failed to add assistant message to memory: {} - request_id: {}",
            e,
            request_id
        );
    }

    // Write assistant message to session history
    write_assistant_to_session(&state, &session_id, &request, &final_content, is_privacy).await;

    // Trigger async memory recording for plan result (fire-and-forget)
    if !is_privacy && state.has_memory_writer() {
        let state_clone = Arc::clone(&state);
        let chat_url = format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
        let api_key = chat_server.api_key.clone();
        let model = model_name.clone();
        let user_msg = user_request.clone();
        let plan_result = final_content.clone();
        tokio::spawn(async move {
            trigger_plan_memory(
                &state_clone,
                &chat_url,
                api_key.as_deref(),
                &model,
                &user_msg,
                &plan_result,
            )
            .await;
        });
    }

    // Finalize trace
    trace.finalize(TraceStatus::Success);
    dual_info!("Plan trace: {}", trace.summary());
    dual_debug!(
        "Plan trace details:\n{}",
        serde_json::to_string_pretty(&trace).unwrap_or_default()
    );

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

    // Calculate execution statistics for finish event
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

    // Emit finish event
    let execution_summary = ExecutionSummary {
        subtask_count: trace.subtask_traces.len(),
        completed_count,
        failed_count,
        tool_call_count,
        duration_ms: trace.total_duration.as_millis() as u64,
    };
    emitter
        .emit_finish(&trace.total_tokens, "stop", None, Some(execution_summary))
        .await;

    // Send text events for final content
    let text_chunks = gen_chunks_with_formatting(&final_content, 10);
    for chunk in text_chunks {
        let event = format_sse_event("text", &TextEvent { content: chunk });
        if event_sender.send(event).await.is_err() {
            dual_info!("Client disconnected - request_id: {}", request_id);
            return Ok(());
        }
    }

    // Send done marker
    let _ = event_sender.send("data: [DONE]\n\n".to_string()).await;

    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Gets the chat server for making LLM requests.
///
/// Routes to either the normal `chat` or `privacy_chat` server group
/// based on the specified `kind`.
async fn get_chat_server(
    state: &Arc<AppState>,
    request_id: &str,
    kind: ServerKind,
) -> ServerResult<crate::server::TargetServerInfo> {
    let servers = state.server_group.read().await;
    let chat_servers = match servers.get(&kind) {
        Some(servers) => servers,
        None => {
            let err_msg = format!("No {} server available", kind);
            dual_error!("{} - request_id: {}", err_msg, request_id);
            return Err(ServerError::Operation(err_msg));
        }
    };

    match chat_servers.next().await {
        Ok(target_server_info) => Ok(target_server_info),
        Err(e) => {
            let err_msg = format!("Failed to get the {} server: {e}", kind);
            dual_error!("{} - request_id: {}", err_msg, request_id);
            Err(ServerError::Operation(err_msg))
        }
    }
}

/// Write assistant message to JSONL session history.
///
/// This is a no-op if session_id or session_writer is absent.
async fn write_assistant_to_session(
    state: &Arc<AppState>,
    session_id: &Option<String>,
    request: &ChatCompletionRequest,
    content: &str,
    privacy_mode: bool,
) {
    if let Some(sid) = session_id
        && let Some(writer) = &state.session_writer
    {
        let uid = request.user.as_deref().unwrap_or("anonymous");
        let model_name = request
            .model
            .clone()
            .unwrap_or_else(|| "default".to_string());
        let seq = writer.next_sequence(uid).await;
        let record = crate::session::types::SessionRecord::Message {
            version: crate::session::types::JSONL_FORMAT_VERSION,
            role: "assistant".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now(),
            message_id: format!("msg_{}", uuid::Uuid::new_v4()),
            sequence: seq,
            tokens: None,
            tool_calls: None,
            privacy_mode,
        };
        if let Err(e) = writer.append_message(uid, sid, &model_name, record).await {
            dual_warn!("Failed to write assistant message to session: {}", e);
        }
    }
}

/// Determines the chat server kind based on the `X-Privacy-Mode` header.
fn resolve_chat_server_kind(headers: &HeaderMap) -> ServerKind {
    let is_privacy = headers
        .get("x-privacy-mode")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    if is_privacy {
        ServerKind::privacy_chat
    } else {
        ServerKind::chat
    }
}

/// Runs the full privacy detection pipeline: rules first, model fallback.
///
/// 1. Determines whether model detection is needed based on the detection mode.
/// 2. If needed, calls the privacy chat LLM for model-based detection.
/// 3. Merges rules and model results via `detect_with_model()`.
/// 4. If the model call fails at any point, gracefully falls back to rules-only.
async fn run_privacy_detection(
    state: &Arc<AppState>,
    detector: &crate::services::privacy::PrivacyDetector,
    text: &str,
    cancel_token: &CancellationToken,
) -> Result<
    crate::services::privacy::PrivacyDetectionResult,
    crate::services::privacy::PrivacyDetectionError,
> {
    use crate::services::privacy::DetectionMode;

    let needs_model = matches!(
        detector.config().mode,
        DetectionMode::RulesThenModel | DetectionMode::ModelOnly | DetectionMode::Combined
    );

    if !needs_model {
        dual_debug!("Detection mode is rules_only, skipping model detection");
        return detector.detect_with_model(text, None);
    }

    // Attempt model-based detection via privacy_chat LLM
    dual_debug!("Attempting LLM-based privacy detection via privacy_chat service");
    let model_response = match call_privacy_model(state, detector, text, cancel_token).await {
        Ok(response) => Some(response),
        Err(e) => {
            dual_warn!(
                "Privacy model detection failed: {}, falling back to rules-only",
                e
            );
            None
        }
    };

    detector.detect_with_model(text, model_response.as_deref())
}

/// Calls the privacy_chat LLM for privacy detection.
///
/// Gets a privacy_chat server, builds the request from `detector.prepare_model_request()`,
/// sends it, and returns the model's text response.
///
/// All errors are returned as `String` — callers treat any failure as
/// "model unavailable" and fall back to rules-only detection.
async fn call_privacy_model(
    state: &Arc<AppState>,
    detector: &crate::services::privacy::PrivacyDetector,
    text: &str,
    cancel_token: &CancellationToken,
) -> Result<String, String> {
    // 1. Get a privacy_chat server (must NOT use normal chat server)
    let request_id = gen_chat_id();
    let server_info = get_chat_server(state, &request_id, ServerKind::privacy_chat)
        .await
        .map_err(|e| format!("No privacy_chat server available for model detection: {e}"))?;

    // 2. Build LLM request
    let prompt = detector.prepare_model_request(text);
    let user_message = ChatCompletionRequestMessage::new_user_message(
        ChatCompletionUserMessageContent::Text(prompt),
        None,
    );
    let chat_request = ChatCompletionRequestBuilder::new(&[user_message])
        .with_max_completion_tokens(512)
        .build();

    // 3. Build HTTP request
    let url = format!("{}/chat/completions", server_info.url.trim_end_matches('/'));
    let mut request = reqwest::Client::new()
        .post(&url)
        .header(CONTENT_TYPE, "application/json")
        .json(&chat_request);

    if let Some(ref api_key) = server_info.api_key
        && !api_key.is_empty()
    {
        request = request.header(AUTHORIZATION, format!("Bearer {}", api_key));
    }

    // 4. Send request with cancellation support
    let response = select! {
        result = request.send() => {
            result.map_err(|e| format!("Privacy model LLM request failed: {e}"))?
        }
        _ = cancel_token.cancelled() => {
            return Err("Privacy model detection cancelled".to_string());
        }
    };

    // 5. Check HTTP status
    if !response.status().is_success() {
        return Err(format!(
            "Privacy model LLM returned HTTP {}",
            response.status()
        ));
    }

    // 6. Parse response
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read privacy model response: {e}"))?;

    let completion: ChatCompletionObject = serde_json::from_slice(&bytes)
        .map_err(|e| format!("Failed to parse privacy model response: {e}"))?;

    // 7. Extract content
    completion
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .ok_or_else(|| "Privacy model LLM returned empty content".to_string())
}

/// Detects privacy content and confirms with user via HITL if needed.
///
/// This function implements the smart privacy detection flow:
/// 1. Check if X-Privacy-Mode header is set (backward compatibility)
/// 2. If not, run privacy detection on the user message
/// 3. If privacy content detected, create HITL confirmation request
/// 4. Wait for user response and return appropriate ServerKind
///
/// # Arguments
/// * `state` - Application state containing privacy detector
/// * `headers` - HTTP headers (for X-Privacy-Mode check)
/// * `user_message` - The user's message to analyze
/// * `conv_id` - Conversation ID for HITL request
/// * `user_id` - User ID for HITL request
/// * `cancel_token` - Cancellation token for async operations
///
/// # Returns
/// * `ServerKind::privacy_chat` if privacy mode should be used
/// * `ServerKind::chat` for normal mode
async fn detect_and_confirm_privacy(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    user_message: Option<&str>,
    conv_id: &str,
    user_id: &str,
    cancel_token: &CancellationToken,
) -> ServerResult<ServerKind> {
    // 1. Check if X-Privacy-Mode header is explicitly set (backward compatibility)
    let manual_privacy = headers
        .get("x-privacy-mode")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "true");

    if manual_privacy {
        dual_debug!("Privacy mode enabled via X-Privacy-Mode header");
        return Ok(ServerKind::privacy_chat);
    }

    // 2. Check if privacy detection is enabled and we have a message to analyze
    let Some(detector) = state.privacy_detector() else {
        dual_debug!("Privacy detection not configured, using normal chat");
        return Ok(ServerKind::chat);
    };

    let Some(message) = user_message else {
        dual_debug!("No user message to analyze, using normal chat");
        return Ok(ServerKind::chat);
    };

    // 3. Run privacy detection (rules first, model fallback)
    let detection_result = match run_privacy_detection(state, detector, message, cancel_token).await
    {
        Ok(result) => result,
        Err(crate::services::privacy::PrivacyDetectionError::NotEnabled) => {
            dual_debug!("Privacy detection not enabled");
            return Ok(ServerKind::chat);
        }
        Err(e) => {
            dual_warn!("Privacy detection failed: {}, using normal chat", e);
            return Ok(ServerKind::chat);
        }
    };

    // 5. If no privacy content detected, use normal chat
    if !detection_result.is_private {
        dual_debug!("No privacy content detected, using normal chat");
        return Ok(ServerKind::chat);
    }

    dual_info!(
        "Privacy content detected (confidence: {:.2}), requesting user confirmation",
        detection_result.confidence
    );

    // 6. Check if HITL is available
    let Some(hitl_manager) = hitl::global() else {
        dual_warn!("HITL not available, defaulting to privacy mode for safety");
        return Ok(ServerKind::privacy_chat);
    };

    // 7. Convert detection patterns to HITL format
    let detected_patterns: Vec<DetectedPrivacyPattern> = detection_result
        .matched_patterns
        .iter()
        .map(|p| {
            let category_str = format!("{:?}", p.category).to_lowercase();
            DetectedPrivacyPattern {
                category: category_str,
                description: p.pattern_type.clone(),
                masked_text: p.matched_text.clone(),
            }
        })
        .collect();

    // 8. Create summarized/masked query for display
    let query_summary = if message.len() > 100 {
        let mut end = 100;
        while !message.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &message[..end])
    } else {
        message.to_string()
    };

    // 9. Determine detection method string
    let detection_method = match detection_result.detection_method {
        crate::services::privacy::DetectionMethod::RuleBased => "rule_based",
        crate::services::privacy::DetectionMethod::ModelInference => "model_inference",
        crate::services::privacy::DetectionMethod::Combined => "combined",
    };

    // 10. Generate recommendation
    let recommendation = if detection_result.confidence > 0.8 {
        "建议使用隐私模式以保护您的敏感信息"
    } else {
        "检测到可能包含敏感信息，您可以选择是否使用隐私模式"
    };

    // 11. Create HITL privacy confirmation request
    let request = match hitl_manager
        .create_privacy_confirmation_request(
            &query_summary,
            detected_patterns,
            detection_method,
            detection_result.confidence,
            recommendation,
            conv_id,
            user_id,
        )
        .await
    {
        Ok(req) => req,
        Err(e) => {
            dual_error!("Failed to create HITL privacy confirmation: {}", e);
            // Default to privacy mode for safety on error
            return Ok(ServerKind::privacy_chat);
        }
    };

    let hitl_request_id = request.id.clone();
    dual_info!(
        "Waiting for user privacy mode choice - request_id: {}",
        hitl_request_id
    );

    // 12. Wait for user response (with cancellation support)
    let response = match hitl_manager
        .wait_for_response_with_cancel(&hitl_request_id, Some(cancel_token))
        .await
    {
        Ok(resp) => resp,
        Err(hitl::HitlError::Cancelled(_)) => {
            dual_info!("Privacy confirmation cancelled, using normal chat");
            return Ok(ServerKind::chat);
        }
        Err(hitl::HitlError::Timeout(behavior)) => {
            dual_warn!("Privacy confirmation timed out (behavior: {:?})", behavior);
            // Default to privacy mode on timeout for safety
            return Ok(ServerKind::privacy_chat);
        }
        Err(e) => {
            dual_error!("HITL wait failed: {}, defaulting to privacy mode", e);
            return Ok(ServerKind::privacy_chat);
        }
    };

    // 13. Process user response
    match response {
        HitlResponse::PrivacyModeChoice {
            use_privacy_mode,
            remember_choice,
        } => {
            if remember_choice {
                dual_debug!(
                    "User chose to remember privacy mode choice: {}",
                    use_privacy_mode
                );
                // TODO: Store user preference for session
            }

            if use_privacy_mode {
                dual_info!("User confirmed privacy mode");
                Ok(ServerKind::privacy_chat)
            } else {
                dual_info!("User chose normal mode");
                Ok(ServerKind::chat)
            }
        }
        HitlResponse::Abort { reason } => {
            dual_info!("User aborted privacy confirmation: {:?}", reason);
            // Default to normal chat on abort
            Ok(ServerKind::chat)
        }
        _ => {
            dual_warn!("Unexpected HITL response for privacy confirmation, using privacy mode");
            Ok(ServerKind::privacy_chat)
        }
    }
}

/// Gets available tools from MCP services and internal tools.
///
/// Returns both MCP tools (from registered MCP servers) and internal tools
/// (like `skill_run_script`). MCP tools include their full parameter schemas
/// for proper LLM tool calling.
async fn get_available_tools(state: &Arc<AppState>) -> Vec<ToolDescription> {
    let mut tools = Vec::new();

    // Add MCP tools from config (which has full tool info including input_schema)
    if let Some(mcp_config) = state.config.read().await.mcp.as_ref() {
        for server_config in mcp_config.server.tool_servers.iter() {
            if server_config.enable
                && let Some(ref mcp_tools) = server_config.tools
            {
                for mcp_tool in mcp_tools {
                    let name = format_mcp_tool_name(
                        server_config
                            .server_name
                            .as_deref()
                            .unwrap_or(&server_config.name),
                        &mcp_tool.name,
                    );
                    tools.push(ToolDescription {
                        name,
                        description: mcp_tool
                            .description
                            .as_ref()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                format!(
                                    "Tool {} from {}",
                                    mcp_tool.name,
                                    server_config
                                        .server_name
                                        .as_deref()
                                        .unwrap_or(&server_config.name)
                                )
                            }),
                        parameters: Some(serde_json::Value::Object(
                            (*mcp_tool.input_schema).clone(),
                        )),
                    });
                }
            }
        }
    }

    // Add internal tools (only available when skills are loaded)
    if SkillRegistry::global().is_ok() {
        tools.push(ToolDescription {
            name: internal_tool_name(SKILL_RUN_SCRIPT_TOOL),
            description: "Execute a script from an active skill. Use this tool to run scripts in the skill's scripts/ directory.".to_string(),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "script_name": {
                        "type": "string",
                        "description": "Name of the script file to execute (e.g., 'process.js', 'export.py')"
                    },
                    "args": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional command line arguments to pass to the script"
                    }
                },
                "required": ["script_name"]
            })),
        });
        tools.push(ToolDescription {
            name: internal_tool_name(SKILL_LOAD_ASSET_TOOL),
            description: "Load an asset file from the active skill's assets/ directory. Supports template variable replacement and JSON/YAML parsing.".to_string(),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "asset_name": {
                        "type": "string",
                        "description": "Name of the asset file to load (e.g., 'template.md', 'config.json')"
                    },
                    "variables": {
                        "type": "object",
                        "description": "Optional variables for template replacement. Use {{variable}} syntax in the template."
                    },
                    "parse_as": {
                        "type": "string",
                        "enum": ["json", "yaml", "markdown"],
                        "description": "Optional format to parse the content as. If not specified, returns raw content."
                    }
                },
                "required": ["asset_name"]
            })),
        });
    }

    // Add Lantai knowledge base tools
    if state.has_lantai() {
        tools.push(ToolDescription {
            name: internal_tool_name(LANTAI_SEARCH_TOOL),
            description: "Search the knowledge base for relevant information. Returns semantically matched document chunks from indexed markdown files.".to_string(),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query to find relevant knowledge"
                    },
                    "limit": {
                        "type": "integer",
                        "description": "Maximum number of results to return (default: 5)",
                        "default": 5
                    }
                },
                "required": ["query"]
            })),
        });
        tools.push(ToolDescription {
            name: internal_tool_name(LANTAI_STATS_TOOL),
            description: "Get statistics about the knowledge base index, including file count, chunk count, and cached embeddings count.".to_string(),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {}
            })),
        });

        // Memory tools (require MemoryWriter)
        if state.has_memory_writer() {
            tools.push(ToolDescription {
                name: internal_tool_name(LANTAI_WRITE_MEMORY_TOOL),
                description: "Save important information to the knowledge base. Default category is 'daily' (session notes). Use 'core' for long-term knowledge (user preferences, key decisions), 'experience' for practical tips and problem-solving patterns. Core entries are automatically dated.".to_string(),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "content": {
                            "type": "string",
                            "description": "Information to save"
                        },
                        "category": {
                            "type": "string",
                            "enum": ["daily", "core", "experience"],
                            "description": "Memory category (default: 'daily')",
                            "default": "daily"
                        },
                        "heading": {
                            "type": "string",
                            "description": "Section heading for MEMORY.md / EXPERIENCE.md organization"
                        }
                    },
                    "required": ["content"]
                })),
            });
            tools.push(ToolDescription {
                name: internal_tool_name(LANTAI_UPDATE_MEMORY_TOOL),
                description: "Update an existing section in MEMORY.md or EXPERIENCE.md. Replaces the entire content under the specified heading. Use to fix outdated information or merge duplicates. Only works on 'core' and 'experience' categories.".to_string(),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "category": {
                            "type": "string",
                            "enum": ["core", "experience"],
                            "description": "Target file category"
                        },
                        "heading": {
                            "type": "string",
                            "description": "Exact heading of the section to update"
                        },
                        "content": {
                            "type": "string",
                            "description": "New content to replace the section body"
                        }
                    },
                    "required": ["category", "heading", "content"]
                })),
            });
            tools.push(ToolDescription {
                name: internal_tool_name(LANTAI_DELETE_MEMORY_TOOL),
                description: "Delete an entire section from MEMORY.md or EXPERIENCE.md by heading. Use to remove outdated or irrelevant sections. Only works on 'core' and 'experience' categories.".to_string(),
                parameters: Some(serde_json::json!({
                    "type": "object",
                    "properties": {
                        "category": {
                            "type": "string",
                            "enum": ["core", "experience"],
                            "description": "Target file category"
                        },
                        "heading": {
                            "type": "string",
                            "description": "Exact heading of the section to delete"
                        }
                    },
                    "required": ["category", "heading"]
                })),
            });
        }
    }

    // Add Sub-Agent tools
    for tool_desc in all_subagent_tool_descriptions() {
        tools.push(ToolDescription {
            name: tool_desc.name,
            description: tool_desc.description,
            ..Default::default()
        });
    }

    tools
}

/// Executes a single subtask using a React loop.
///
/// This function implements a React (Reason + Act) loop for executing subtasks,
/// allowing the LLM to iteratively think, call tools, observe results, and
/// produce a final answer.
///
/// Supports two-phase skill loading:
/// - Phase 1: Skills summaries are shown, LLM can request a skill via `<use_skill>` tag
/// - Phase 2: Full skill content is loaded and injected into context
#[allow(clippy::too_many_arguments)]
async fn execute_subtask_with_react(
    state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    headers: &HeaderMap,
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    available_tools: &[ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    conv_id: Option<&str>,
    user_id: &str,
    timeout: Duration,
    max_iterations: u32,
    max_tools_per_iteration: usize,
    tool_call_max_retries: u32,
    tool_call_retry_delay_ms: u64,
    cancel_token: &CancellationToken,
    request_id: &str,
    subtask_trace: &mut SubtaskTrace,
    model: &str,
    emitter: &dyn EventEmitter,
    plan_pause_tracker: Option<Arc<AtomicU64>>,
    file_attachments: &[FileAttachmentInfo],
) -> ServerResult<String> {
    let start_time = Instant::now();
    // Track total time spent in tool execution (including HITL wait).
    // This is subtracted from elapsed time in the timeout check so that
    // human-in-the-loop approval time does not count toward the subtask timeout.
    let mut tool_pause_duration = Duration::ZERO;
    let tool_call_retry_delay = Duration::from_millis(tool_call_retry_delay_ms);

    // Get max_reference_size from config
    let max_reference_size = state
        .config
        .read()
        .await
        .skill
        .as_ref()
        .map(|s| s.max_reference_size)
        .unwrap_or(102400); // Default 100KB

    // Create Sub-Agent manager and context for tool execution
    let subagent_config = SubAgentSystemConfig::default_enabled();
    let subagent_manager = Arc::new(SubAgentManager::new(subagent_config));
    let subagent_ctx = SubAgentToolContext {
        chat_server: chat_server.clone(),
        headers: headers.clone(),
        manager: subagent_manager,
        available_tools: available_tools.to_vec(),
        model: model.to_string(),
        cancel_token: cancel_token.clone(),
    };

    // Track active skills for Phase 2 (supports multi-skill activation)
    let mut active_skills: Vec<LoadedSkill> = Vec::new();

    // Build initial messages for React loop (Phase 1: no active skills yet)
    let mut messages = build_context_for_react(
        subtask,
        previous_results,
        available_tools,
        skills_summaries,
        &[], // No active skills in initial context
        max_reference_size,
        file_attachments,
    )
    .await;

    // Inject memory context into system prompt (if MemoryWriter is available)
    if state.has_memory_writer() {
        let lantai_cfg = state.config.read().await.lantai.clone();
        let context_injection_enabled = lantai_cfg
            .as_ref()
            .map(|l| l.auto_memory.context_injection)
            .unwrap_or(true);
        if context_injection_enabled && let Some(writer) = state.memory_writer() {
            let max_chars = lantai_cfg
                .as_ref()
                .map(|l| l.auto_memory.max_context_chars)
                .unwrap_or(2000);
            let memory_context = load_memory_context(writer, max_chars).await;
            if !memory_context.is_empty() {
                // Append memory context to the system message
                if let Some(ChatCompletionRequestMessage::System(sys_msg)) = messages.first_mut() {
                    let new_content = format!(
                        "{}\n\n## Recent Memory Context\n{}\n\nYou have a knowledge base with experience notes and historical logs. Use `internal__lantai_search` to look up past solutions or recent conversations when relevant.",
                        sys_msg.content(),
                        memory_context
                    );
                    *sys_msg = ChatCompletionSystemMessage::new(new_content, None);
                }
            }
        }
    }

    // Memory checkpoint: track whether we've already triggered a checkpoint
    // to avoid repeated saves during the same React loop execution.
    let mut checkpoint_triggered = false;

    // React loop
    let mut iteration_count: u32 = 0;

    loop {
        let iter_start = Instant::now();

        // Check iteration limit
        iteration_count += 1;
        if iteration_count > max_iterations {
            dual_warn!(
                "Subtask {} React loop exceeded maximum iterations ({}) - request_id: {}",
                subtask.id,
                max_iterations,
                request_id
            );
            subtask_trace.set_react_status(TraceStatus::MaxIterationsExceeded);
            return Err(ServerError::MaxIterationsExceeded(max_iterations));
        }

        // Check timeout (excludes time spent in tool execution / HITL wait)
        let effective_elapsed = start_time.elapsed().saturating_sub(tool_pause_duration);
        if effective_elapsed > timeout {
            dual_warn!(
                "Subtask {} React loop timeout after {:?} (effective {:?}, tool pause {:?}) - request_id: {}",
                subtask.id,
                start_time.elapsed(),
                effective_elapsed,
                tool_pause_duration,
                request_id
            );
            subtask_trace.timeout();
            return Err(ServerError::SubtaskTimeout {
                subtask_id: subtask.id,
                timeout_secs: timeout.as_secs(),
            });
        }

        // Check cancellation
        if cancel_token.is_cancelled() {
            return Err(ServerError::Operation(
                "Request was cancelled by client".to_string(),
            ));
        }

        // Initialize iteration trace
        let mut iter_trace = IterationTrace::new(iteration_count);

        dual_debug!(
            "Subtask {} React iteration {}/{} - request_id: {}",
            subtask.id,
            iteration_count,
            max_iterations,
            request_id
        );

        // Build and send request to LLM
        let url = format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
        let mut client = reqwest::Client::new().post(&url);
        client = client.header(CONTENT_TYPE, "application/json");

        if let Some(api_key) = &chat_server.api_key
            && !api_key.is_empty()
        {
            let auth_info = if api_key.starts_with("Bearer ") {
                api_key.clone()
            } else {
                format!("Bearer {api_key}")
            };
            client = client.header(AUTHORIZATION, auth_info);
        } else if let Some(auth) = headers.get("authorization")
            && let Ok(auth_str) = auth.to_str()
        {
            client = client.header(AUTHORIZATION, auth_str);
        }

        // Build request with tools (filtered by active skills if any)
        let allowed_patterns = if active_skills.is_empty() {
            None
        } else {
            Some(SkillInjector::merge_allowed_tools(&active_skills))
        };
        let tools_json = build_tools_json(available_tools, allowed_patterns.as_deref());
        let request_json = serde_json::json!({
            "model": model,
            "messages": messages,
            "tools": tools_json,
            "stream": false
        });

        // Send request with cancellation support
        let ds_response = select! {
            response = client.json(&request_json).send() => {
                response.map_err(|e| ServerError::Operation(format!("Failed to forward request: {e}")))
            }
            _ = cancel_token.cancelled() => {
                return Err(ServerError::Operation("Request was cancelled by client".to_string()));
            }
        }?;

        // Parse response — read body as text first to enable detailed error logging
        let response_text = ds_response
            .text()
            .await
            .map_err(|e| ServerError::Operation(format!("Failed to read response body: {e}")))?;
        let chat_completion: ChatCompletionObject =
            serde_json::from_str(&response_text).map_err(|e| {
                let preview = if response_text.len() > 500 {
                    let mut end = 500;
                    while !response_text.is_char_boundary(end) {
                        end -= 1;
                    }
                    &response_text[..end]
                } else {
                    &response_text
                };
                dual_error!(
                    "Failed to parse LLM response: {}. Body preview: {}",
                    e,
                    preview
                );
                ServerError::Operation(format!("Failed to parse response: {e}"))
            })?;

        // Record token usage
        let usage = &chat_completion.usage;
        iter_trace.llm_tokens = TokenUsage::new(usage.prompt_tokens, usage.completion_tokens);

        // Memory checkpoint: when prompt_tokens exceeds threshold, async-save conversation context
        if !checkpoint_triggered && state.has_memory_writer() {
            let config_guard = state.config.read().await;
            let model_ctx_size = config_guard
                .chat
                .as_ref()
                .map(|c| c.model_context_size)
                .unwrap_or(0);
            let ratio = config_guard
                .lantai
                .as_ref()
                .map(|l| l.auto_memory.checkpoint_token_ratio)
                .unwrap_or(0.75);
            drop(config_guard);

            if model_ctx_size > 0 {
                let threshold = (model_ctx_size as f64 * ratio as f64) as u64;
                if usage.prompt_tokens as u64 >= threshold {
                    checkpoint_triggered = true;
                    dual_info!(
                        "Memory checkpoint triggered: prompt_tokens={} >= threshold={} ({}*{:.2}) - request_id: {}",
                        usage.prompt_tokens,
                        threshold,
                        model_ctx_size,
                        ratio,
                        request_id
                    );
                    // Fire-and-forget: extract key info from conversation and save to daily log
                    if let Some(writer) = state.memory_writer() {
                        let writer = writer.clone();
                        let chat_url =
                            format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
                        let chat_api_key = chat_server.api_key.clone();
                        let checkpoint_model = model.to_string();
                        // Collect recent user/assistant content from messages for summarization
                        let conversation_snippet = extract_conversation_snippet(&messages, 3000);
                        let rid = request_id.to_string();
                        tokio::spawn(async move {
                            if let Err(e) = trigger_memory_checkpoint(
                                &writer,
                                &chat_url,
                                chat_api_key.as_deref(),
                                &checkpoint_model,
                                &conversation_snippet,
                            )
                            .await
                            {
                                tracing::warn!(
                                    "Memory checkpoint failed (request_id: {}): {}",
                                    rid,
                                    e
                                );
                            }
                        });
                    }
                }
            }
        }

        // Check for tool calls - support both OpenAI JSON format and XML JSON-embedded format
        let json_tool_calls = &chat_completion.choices[0].message.tool_calls;
        let content = chat_completion.choices[0].message.content.as_ref();

        // Priority: OpenAI JSON format > XML JSON-embedded format
        let requires_tool_call = if !json_tool_calls.is_empty() {
            true
        } else if let Some(content) = content {
            // Check for XML tool call with JSON-embedded format
            // Must have <action> tag, not have <final_answer> tag, and be valid JSON
            has_action_tag(content)
                && !has_final_answer_tag(content)
                && extract_xml_tool_call(content).is_some()
        } else {
            false
        };

        if requires_tool_call {
            // Check which format is used
            if !json_tool_calls.is_empty() {
                // === OpenAI JSON format tool calls ===
                let tool_calls_to_execute = if json_tool_calls.len() > max_tools_per_iteration {
                    &json_tool_calls[..max_tools_per_iteration]
                } else {
                    json_tool_calls.as_slice()
                };

                // Extract thought from content if present
                if let Some(content) = content {
                    if let Some(thought) = extract_thought(content) {
                        dual_info!("💭 Subtask {} Thought: {}", subtask.id, thought);
                        iter_trace.thought = Some(thought.clone());

                        // Emit thought event
                        emitter
                            .emit_thought(
                                &thought,
                                ThoughtStatus::Done,
                                Some(subtask.id),
                                Some(iteration_count),
                            )
                            .await;
                    }
                    if let Some(action) = extract_action(content) {
                        dual_info!("🔧 Subtask {} Action: {}", subtask.id, action);
                        iter_trace.action = Some(action);
                    }
                }

                // Execute tool calls
                for tool_call in tool_calls_to_execute {
                    // Emit tool_call event before execution
                    let tool_args: serde_json::Value =
                        serde_json::from_str(&tool_call.function.arguments)
                            .unwrap_or(serde_json::json!({}));
                    let (server_name, _tool_name) = parse_mcp_tool_name(&tool_call.function.name)
                        .unwrap_or(("unknown", &tool_call.function.name));
                    emitter
                        .emit_tool_call(
                            &tool_call.id,
                            &tool_call.function.name,
                            &tool_args,
                            Some(server_name),
                            Some(subtask.id),
                        )
                        .await;

                    let tool_call_start = Instant::now();
                    let tool_result = execute_tool_call(
                        state,
                        tool_call,
                        &active_skills,
                        conv_id,
                        user_id,
                        Some(subtask.id),
                        tool_call_max_retries,
                        tool_call_retry_delay,
                        request_id,
                        cancel_token,
                        &mut iter_trace,
                        Some(&subagent_ctx),
                        emitter,
                    )
                    .await;

                    // Emit tool_result event after execution
                    let tool_duration = tool_call_start.elapsed();
                    // Exclude tool execution time (incl. HITL wait) from subtask timeout
                    tool_pause_duration += tool_duration;
                    // Also report to plan-level time budget tracker
                    if let Some(ref tracker) = plan_pause_tracker {
                        tracker.fetch_add(tool_duration.as_nanos() as u64, Ordering::Relaxed);
                    }
                    match &tool_result {
                        Ok(result) => {
                            emitter
                                .emit_tool_result(
                                    &tool_call.id,
                                    result,
                                    false,
                                    Some(tool_duration),
                                    Some(subtask.id),
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
                                    Some(subtask.id),
                                )
                                .await;
                        }
                    }
                    let tool_result = tool_result?;

                    // Format observation
                    let observation = format!("<observation>{}</observation>", tool_result);
                    iter_trace.observation = Some(tool_result.clone());

                    // Append assistant message with tool call
                    messages.push(ChatCompletionRequestMessage::Assistant(
                        ChatCompletionAssistantMessage::new(
                            chat_completion.choices[0].message.content.clone(),
                            None,
                            Some(vec![tool_call.clone()]),
                        ),
                    ));

                    // Append tool result message
                    messages.push(ChatCompletionRequestMessage::Tool(
                        ChatCompletionToolMessage::new(&observation, &tool_call.id),
                    ));
                }
            } else if let Some(content) = content {
                // === Check for skill request FIRST (before action tags) ===
                // If LLM outputs both <use_skill> and <action> in the same response,
                // prioritize skill loading and ignore the action tag.
                let has_skill_request =
                    active_skills.is_empty() && SkillDetector::detect_first(content).is_some();

                // === XML JSON-embedded format tool call ===
                // Skip if there's a skill request in the same response
                if !has_skill_request && let Some(xml_tool_call) = extract_xml_tool_call(content) {
                    // Extract thought
                    if let Some(thought) = extract_thought(content) {
                        dual_info!("💭 Subtask {} Thought: {}", subtask.id, thought);
                        iter_trace.thought = Some(thought.clone());

                        // Emit thought event
                        emitter
                            .emit_thought(
                                &thought,
                                ThoughtStatus::Done,
                                Some(subtask.id),
                                Some(iteration_count),
                            )
                            .await;
                    }

                    dual_info!(
                        "🔧 Subtask {} Action (XML JSON): {}",
                        subtask.id,
                        xml_tool_call.tool_name
                    );
                    iter_trace.action = Some(xml_tool_call.tool_name.clone());

                    // Construct ToolCall structure for execution
                    let tool_call = endpoints::chat::ToolCall {
                        id: format!("xml_call_{}", gen_chat_id()),
                        ty: "function".to_string(),
                        function: endpoints::chat::Function {
                            name: xml_tool_call.tool_name.clone(),
                            arguments: xml_tool_call.arguments.clone(),
                        },
                    };

                    // Parse tool arguments for event emission
                    let tool_args: serde_json::Value =
                        serde_json::from_str(&xml_tool_call.arguments)
                            .unwrap_or(serde_json::json!({}));
                    let (server_name, _tool_name) = parse_mcp_tool_name(&xml_tool_call.tool_name)
                        .unwrap_or(("unknown", &xml_tool_call.tool_name));

                    // Emit tool_call event before execution
                    emitter
                        .emit_tool_call(
                            &tool_call.id,
                            &xml_tool_call.tool_name,
                            &tool_args,
                            Some(server_name),
                            Some(subtask.id),
                        )
                        .await;

                    // Execute tool call
                    let tool_call_start = Instant::now();
                    let tool_result = execute_tool_call(
                        state,
                        &tool_call,
                        &active_skills,
                        conv_id,
                        user_id,
                        Some(subtask.id),
                        tool_call_max_retries,
                        tool_call_retry_delay,
                        request_id,
                        cancel_token,
                        &mut iter_trace,
                        Some(&subagent_ctx),
                        emitter,
                    )
                    .await;

                    // Emit tool_result event after execution
                    let tool_duration = tool_call_start.elapsed();
                    // Exclude tool execution time (incl. HITL wait) from subtask timeout
                    tool_pause_duration += tool_duration;
                    // Also report to plan-level time budget tracker
                    if let Some(ref tracker) = plan_pause_tracker {
                        tracker.fetch_add(tool_duration.as_nanos() as u64, Ordering::Relaxed);
                    }
                    match &tool_result {
                        Ok(result) => {
                            emitter
                                .emit_tool_result(
                                    &tool_call.id,
                                    result,
                                    false,
                                    Some(tool_duration),
                                    Some(subtask.id),
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
                                    Some(subtask.id),
                                )
                                .await;
                        }
                    }
                    let tool_result = tool_result?;

                    // Format observation
                    let observation = format!("<observation>{}</observation>", tool_result);
                    iter_trace.observation = Some(tool_result.clone());

                    // Append assistant message with tool call
                    messages.push(ChatCompletionRequestMessage::Assistant(
                        ChatCompletionAssistantMessage::new(
                            Some(content.clone()),
                            None,
                            Some(vec![tool_call.clone()]),
                        ),
                    ));

                    // Append tool result message
                    messages.push(ChatCompletionRequestMessage::Tool(
                        ChatCompletionToolMessage::new(&observation, &tool_call.id),
                    ));
                } else if has_skill_request {
                    // Skill request detected - process it (prioritize over action tags)
                    // Extract thought first
                    if let Some(thought) = extract_thought(content) {
                        dual_info!("💭 Subtask {} Thought: {}", subtask.id, thought);
                        iter_trace.thought = Some(thought.clone());

                        emitter
                            .emit_thought(
                                &thought,
                                ThoughtStatus::Done,
                                Some(subtask.id),
                                Some(iteration_count),
                            )
                            .await;
                    }

                    // Process skill request
                    if let Ok(registry) = SkillRegistry::global() {
                        let all_loaded_skills = registry.get_all_loaded().await;
                        let (resolved_skills, removed_skills) =
                            SkillDetector::detect_and_resolve(content, &all_loaded_skills);

                        if !resolved_skills.is_empty() {
                            // Log resolved skills
                            if resolved_skills.len() == 1 {
                                dual_info!(
                                    "🎯 Subtask {} requested skill: {} (ignored concurrent action tag) - request_id: {}",
                                    subtask.id,
                                    resolved_skills[0],
                                    request_id
                                );
                            } else {
                                dual_info!(
                                    "🎯 Subtask {} requested {} skills: [{}] (ignored concurrent action tag) - request_id: {}",
                                    subtask.id,
                                    resolved_skills.len(),
                                    resolved_skills.join(", "),
                                    request_id
                                );
                            }

                            // Log removed skills
                            for (removed, reason) in &removed_skills {
                                dual_warn!(
                                    "⚠️ Skill '{}' removed: {} - request_id: {}",
                                    removed,
                                    reason,
                                    request_id
                                );
                            }

                            // Load all resolved skills
                            let mut loaded_skills_list: Vec<LoadedSkill> = Vec::new();
                            let mut skill_names_loaded: Vec<String> = Vec::new();

                            for skill_name in &resolved_skills {
                                if let Some(loaded_skill) = registry.get(skill_name).await {
                                    dual_info!(
                                        "📖 Loaded skill '{}' for subtask {} - request_id: {}",
                                        skill_name,
                                        subtask.id,
                                        request_id
                                    );
                                    skill_names_loaded.push(skill_name.clone());
                                    loaded_skills_list.push(loaded_skill);
                                } else {
                                    dual_warn!(
                                        "⚠️ Skill '{}' not found, skipping - request_id: {}",
                                        skill_name,
                                        request_id
                                    );
                                }
                            }

                            if !loaded_skills_list.is_empty() {
                                // Record skill request in iteration trace
                                iter_trace.set_skill_request(skill_names_loaded[0].clone(), true);

                                // Record skill activation in subtask trace
                                subtask_trace.set_active_skills(skill_names_loaded.clone());

                                // Store the active skills
                                active_skills = loaded_skills_list;

                                // Rebuild context with the active skills (Phase 2)
                                messages = build_context_for_react(
                                    subtask,
                                    previous_results,
                                    available_tools,
                                    None,
                                    &active_skills,
                                    max_reference_size,
                                    file_attachments,
                                )
                                .await;

                                // Finalize iteration trace and continue loop
                                iter_trace.duration = iter_start.elapsed();
                                subtask_trace.add_iteration(iter_trace);
                                continue;
                            }
                        }
                    }
                }
            }

            // Finalize iteration trace
            iter_trace.duration = iter_start.elapsed();
            subtask_trace.add_iteration(iter_trace);
        } else {
            // No tool calls - check for skill request or final answer
            if let Some(content) = chat_completion.choices[0].message.content.as_ref() {
                // Extract thought if present
                if let Some(thought) = extract_thought(content) {
                    dual_info!("💭 Subtask {} Thought: {}", subtask.id, thought);
                    iter_trace.thought = Some(thought.clone());

                    // Emit thought event
                    emitter
                        .emit_thought(
                            &thought,
                            ThoughtStatus::Done,
                            Some(subtask.id),
                            Some(iteration_count),
                        )
                        .await;
                }

                // Check for skill request (Phase 1 -> Phase 2 transition)
                // Supports multi-skill activation with priority and conflict resolution
                if active_skills.is_empty()
                    && let Ok(registry) = SkillRegistry::global()
                {
                    // Get all loaded skills for priority/conflict resolution
                    let all_loaded_skills = registry.get_all_loaded().await;

                    // Detect and resolve skills (priority sorting + conflict resolution)
                    let (resolved_skills, removed_skills) =
                        SkillDetector::detect_and_resolve(content, &all_loaded_skills);

                    if !resolved_skills.is_empty() {
                        // Log resolved skills
                        if resolved_skills.len() == 1 {
                            dual_info!(
                                "🎯 Subtask {} requested skill: {} - request_id: {}",
                                subtask.id,
                                resolved_skills[0],
                                request_id
                            );
                        } else {
                            dual_info!(
                                "🎯 Subtask {} requested {} skills: [{}] - request_id: {}",
                                subtask.id,
                                resolved_skills.len(),
                                resolved_skills.join(", "),
                                request_id
                            );
                        }

                        // Log removed skills (if any)
                        for (removed, reason) in &removed_skills {
                            dual_warn!(
                                "⚠️ Skill '{}' removed: {} - request_id: {}",
                                removed,
                                reason,
                                request_id
                            );
                        }

                        // Load all resolved skills
                        let mut loaded_skills_list: Vec<LoadedSkill> = Vec::new();
                        let mut skill_names_loaded: Vec<String> = Vec::new();

                        for skill_name in &resolved_skills {
                            if let Some(loaded_skill) = registry.get(skill_name).await {
                                dual_info!(
                                    "📖 Loaded skill '{}' for subtask {} - request_id: {}",
                                    skill_name,
                                    subtask.id,
                                    request_id
                                );
                                skill_names_loaded.push(skill_name.clone());
                                loaded_skills_list.push(loaded_skill);
                            } else {
                                dual_warn!(
                                    "⚠️ Skill '{}' not found, skipping - request_id: {}",
                                    skill_name,
                                    request_id
                                );
                            }
                        }

                        if !loaded_skills_list.is_empty() {
                            // Record skill request in iteration trace
                            // For multi-skill, we record the primary (first) skill
                            iter_trace.set_skill_request(skill_names_loaded[0].clone(), true);

                            // Record skill activation in subtask trace (using set_active_skills for multi-skill)
                            subtask_trace.set_active_skills(skill_names_loaded.clone());

                            // Store the active skills
                            active_skills = loaded_skills_list;

                            // Rebuild context with the active skills (Phase 2)
                            messages = build_context_for_react(
                                subtask,
                                previous_results,
                                available_tools,
                                None, // No need for summaries in Phase 2
                                &active_skills,
                                max_reference_size,
                                file_attachments,
                            )
                            .await;

                            // Finalize iteration trace and continue loop
                            iter_trace.duration = iter_start.elapsed();
                            subtask_trace.add_iteration(iter_trace);
                            continue;
                        }
                    }
                }

                // Check for final answer
                if has_final_answer_tag(content)
                    && let Some(final_answer) = extract_final_answer(content)
                {
                    dual_info!("✅ Subtask {} Final answer: {}", subtask.id, final_answer);

                    iter_trace.duration = iter_start.elapsed();
                    subtask_trace.add_iteration(iter_trace);
                    subtask_trace.set_react_status(TraceStatus::Success);

                    return Ok(final_answer);
                }

                // No final answer tag - treat content as the final answer
                dual_info!(
                    "✅ Subtask {} completed (no final_answer tag): {}",
                    subtask.id,
                    content
                );

                iter_trace.duration = iter_start.elapsed();
                subtask_trace.add_iteration(iter_trace);
                subtask_trace.set_react_status(TraceStatus::Success);

                return Ok(content.clone());
            }

            // No content in response - this shouldn't happen
            iter_trace.duration = iter_start.elapsed();
            subtask_trace.add_iteration(iter_trace);
            return Err(ServerError::Operation(
                "LLM returned empty response".to_string(),
            ));
        }
    }
}

/// Context for executing Sub-Agent tools
#[derive(Clone)]
pub struct SubAgentToolContext {
    pub chat_server: crate::server::TargetServerInfo,
    pub headers: HeaderMap,
    pub manager: Arc<SubAgentManager>,
    pub available_tools: Vec<ToolDescription>,
    pub model: String,
    pub cancel_token: CancellationToken,
}

/// Executes a single tool call with retry logic and HITL support.
///
/// Supports both MCP tools (format: `mcp__{server}__{tool}`) and internal tools
/// (format: `internal__{tool}`).
///
/// # Internal Tools
/// - `internal__skill_run_script`: Execute a script from the active skill
/// - `internal__spawn_sub_agent`: Spawn a new Sub-Agent
/// - `internal__get_sub_agent_result`: Get result from a Sub-Agent
/// - `internal__cancel_sub_agent`: Cancel a running Sub-Agent
///
/// # HITL Support
/// For MCP tools, HITL (Human-in-the-Loop) checking is performed if enabled,
/// allowing users to approve, modify, or reject tool calls based on risk assessment.
#[allow(clippy::too_many_arguments)]
async fn execute_tool_call(
    state: &Arc<AppState>,
    tool_call: &endpoints::chat::ToolCall,
    active_skills: &[LoadedSkill],
    conv_id: Option<&str>,
    user_id: &str,
    subtask_id: Option<usize>,
    max_retries: u32,
    retry_delay: Duration,
    request_id: &str,
    cancel_token: &CancellationToken,
    iter_trace: &mut IterationTrace,
    subagent_ctx: Option<&SubAgentToolContext>,
    emitter: &dyn EventEmitter,
) -> ServerResult<String> {
    let tool_call_start = Instant::now();
    let tool_args: serde_json::Value =
        serde_json::from_str(&tool_call.function.arguments).unwrap_or(serde_json::json!({}));

    // Strip "functions." prefix that some models add to tool names
    let tool_name_raw = &tool_call.function.name;
    let tool_name_cleaned = tool_name_raw
        .strip_prefix("functions.")
        .unwrap_or(tool_name_raw);

    // Check if this is a Sub-Agent tool
    if is_subagent_tool(tool_name_cleaned) {
        return execute_subagent_tool(
            state,
            tool_name_cleaned,
            tool_args,
            request_id,
            iter_trace,
            tool_call_start,
            subagent_ctx,
            emitter,
        )
        .await;
    }

    // Check if this is an internal tool (skill tools, lantai tools)
    if is_internal_tool(tool_name_cleaned) {
        return execute_internal_tool(
            state,
            tool_name_cleaned,
            tool_args,
            active_skills,
            conv_id,
            request_id,
            iter_trace,
            tool_call_start,
        )
        .await;
    }

    // Parse MCP tool name and server name
    let (server_name, tool_name) = parse_mcp_tool_name(tool_name_cleaned).ok_or_else(|| {
        let err_msg = format!("Invalid tool name format: {}", tool_name_cleaned);
        ServerError::Operation(err_msg)
    })?;

    // Initialize tool trace
    let mut tool_trace = ToolCallTrace::new(
        tool_name.to_string(),
        server_name.to_string(),
        tool_args.clone(),
    );

    // Get MCP service
    let services = MCP_SERVICES
        .get()
        .ok_or_else(|| ServerError::Operation("MCP services not initialized".to_string()))?;

    let service_map = services.read().await;
    let service = service_map.get(server_name).ok_or_else(|| {
        let err_msg = format!("MCP server '{}' not found", server_name);
        tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::McpOperation(err_msg)
    })?;

    // Check if HITL is enabled and create the tool caller
    let hitl_caller = hitl::global().map(|m| HitlToolCaller::new(Arc::clone(m)));

    // If HITL is enabled, use check_and_execute for MCP tools
    if let Some(ref hitl_caller) = hitl_caller {
        let mut context =
            HitlToolContext::new(request_id, user_id).with_cancel_token(cancel_token.clone());

        // Add subtask_id to context if available
        if let Some(id) = subtask_id {
            context = context.with_subtask_id(id);
        }

        let tool_name_for_closure = tool_call.function.name.clone();
        let server_name_str = server_name.to_string();
        let max_retries_copy = max_retries;
        let retry_delay_copy = retry_delay;
        let request_id_str = request_id.to_string();

        let result = hitl_caller
            .check_and_execute(&tool_call.function.name, &tool_args, &context, |args| {
                let tool_name = tool_name_for_closure.clone();
                let server_name = server_name_str.clone();
                let max_retries = max_retries_copy;
                let retry_delay = retry_delay_copy;
                let request_id = request_id_str.clone();
                async move {
                    execute_mcp_tool_with_retry(
                        &tool_name,
                        &server_name,
                        &args,
                        max_retries,
                        retry_delay,
                        &request_id,
                    )
                    .await
                }
            })
            .await;

        // Handle HITL result
        match result {
            Ok(hitl_result) => match hitl_result {
                HitlToolResult::Executed(r)
                | HitlToolResult::ExecutedWithoutConfirmation(r)
                | HitlToolResult::Approved(r)
                | HitlToolResult::HitlDisabled(r) => {
                    let final_result = wrap_search_result(server_name, &r, service).await;
                    tool_trace.set_result(final_result.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Ok(final_result)
                }
                HitlToolResult::Modified { result, .. } => {
                    let final_result = wrap_search_result(server_name, &result, service).await;
                    tool_trace.set_result(final_result.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Ok(final_result)
                }
                HitlToolResult::Rejected { reason } => {
                    let err_msg =
                        reason.unwrap_or_else(|| "Tool call rejected by user".to_string());
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::UserInterrupted(err_msg))
                }
                HitlToolResult::Skipped { reason } => {
                    let err_msg = format!("Tool call skipped: {}", reason);
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::Operation(err_msg))
                }
                HitlToolResult::Aborted { reason } => {
                    let err_msg = format!(
                        "Tool call aborted: {}",
                        reason.unwrap_or_else(|| "No reason provided".to_string())
                    );
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::Operation(err_msg))
                }
                HitlToolResult::TimedOut { behavior } => {
                    let err_msg = format!("HITL confirmation timed out (behavior: {})", behavior);
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::Operation(err_msg))
                }
            },
            Err(e) => {
                // Handle Cancelled error specially - treat it as user interruption
                if matches!(e, hitl::HitlError::Cancelled(_)) {
                    let err_msg =
                        "HITL request cancelled due to another subtask rejection".to_string();
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::UserInterrupted(err_msg))
                } else {
                    let err_msg = format!("HITL error: {}", e);
                    tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                    iter_trace.add_tool_call(tool_trace);
                    Err(ServerError::Operation(err_msg))
                }
            }
        }
    } else {
        // No HITL, execute directly with retry
        let result = execute_mcp_tool_with_retry(
            &tool_call.function.name,
            server_name,
            &tool_args,
            max_retries,
            retry_delay,
            request_id,
        )
        .await;

        match result {
            Ok(result_text) => {
                let final_result = wrap_search_result(server_name, &result_text, service).await;
                tool_trace.set_result(final_result.clone(), tool_call_start.elapsed());
                iter_trace.add_tool_call(tool_trace);
                Ok(final_result)
            }
            Err(err_msg) => {
                tool_trace.set_error(err_msg.clone(), tool_call_start.elapsed());
                iter_trace.add_tool_call(tool_trace);
                Err(ServerError::McpOperation(err_msg))
            }
        }
    }
}

/// Execute MCP tool with retry logic (internal helper)
async fn execute_mcp_tool_with_retry(
    full_tool_name: &str,
    server_name: &str,
    args: &serde_json::Value,
    max_retries: u32,
    retry_delay: Duration,
    request_id: &str,
) -> Result<String, String> {
    // Parse the actual tool name
    let (_, tool_name) = parse_mcp_tool_name(full_tool_name)
        .ok_or_else(|| format!("Invalid tool name format: {}", full_tool_name))?;

    // Get MCP services
    let services = MCP_SERVICES
        .get()
        .ok_or_else(|| "MCP services not initialized".to_string())?;

    let service_map = services.read().await;
    let service = service_map
        .get(server_name)
        .ok_or_else(|| format!("MCP server '{}' not found", server_name))?;

    let mut last_error: Option<String> = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            dual_debug!(
                "Retrying tool call {} (attempt {}/{}) - request_id: {}",
                tool_name,
                attempt + 1,
                max_retries + 1,
                request_id
            );
            tokio::time::sleep(retry_delay).await;
        }

        let request_param = CallToolRequestParam {
            name: tool_name.to_string().into(),
            arguments: serde_json::from_value(args.clone()).ok(),
        };

        match service.read().await.raw.call_tool(request_param).await {
            Ok(result) => {
                if result.is_error == Some(true) {
                    let error_detail = result
                        .content
                        .first()
                        .and_then(|c| match &c.raw {
                            RawContent::Text(text) => Some(text.text.clone()),
                            _ => None,
                        })
                        .unwrap_or_else(|| "Unknown error".to_string());
                    dual_warn!(
                        "Tool {} returned error: {} - request_id: {}",
                        tool_name,
                        error_detail,
                        request_id
                    );
                    last_error = Some(format!("Tool returned error: {}", error_detail));
                    continue;
                }

                if !result.content.is_empty()
                    && let RawContent::Text(text) = &result.content[0].raw
                {
                    dual_info!(
                        "Tool call succeeded: {} - request_id: {}",
                        tool_name,
                        request_id
                    );
                    return Ok(text.text.clone());
                }

                return Err("Tool returned empty content".to_string());
            }
            Err(e) => {
                last_error = Some(e.to_string());
                dual_warn!(
                    "Tool call failed (attempt {}/{}): {} - request_id: {}",
                    attempt + 1,
                    max_retries + 1,
                    e,
                    request_id
                );
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "Unknown error".to_string()))
}

/// Wrap search results with context markers if the server is a search server
async fn wrap_search_result(
    server_name: &str,
    result_text: &str,
    service: &tokio::sync::RwLock<crate::mcp::McpService>,
) -> String {
    if SEARCH_MCP_SERVER_NAMES.contains(&server_name) {
        let fallback = if service.read().await.has_fallback_message() {
            service.read().await.fallback_message.clone().unwrap()
        } else {
            DEFAULT_SEARCH_FALLBACK_MESSAGE.to_string()
        };

        format!(
            "Please answer the question based on the information between **---BEGIN CONTEXT---** and **---END CONTEXT---**. Do not use any external knowledge. If the information between **---BEGIN CONTEXT---** and **---END CONTEXT---** is empty, please respond with `{fallback}`. Note that DO NOT use any tools if provided.\n\n---BEGIN CONTEXT---\n\n{context}\n\n---END CONTEXT---",
            fallback = fallback,
            context = result_text,
        )
    } else {
        result_text.to_string()
    }
}

/// Executes a Sub-Agent tool.
///
/// # Supported Sub-Agent Tools
///
/// - `internal__spawn_sub_agent`: Create and execute a new Sub-Agent
/// - `internal__get_sub_agent_result`: Get result from a completed Sub-Agent
/// - `internal__cancel_sub_agent`: Cancel a running Sub-Agent
#[allow(clippy::too_many_arguments)]
async fn execute_subagent_tool(
    state: &Arc<AppState>,
    full_tool_name: &str,
    tool_args: serde_json::Value,
    request_id: &str,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
    subagent_ctx: Option<&SubAgentToolContext>,
    emitter: &dyn EventEmitter,
) -> ServerResult<String> {
    let tool_name = parse_subagent_tool_name(full_tool_name).ok_or_else(|| {
        ServerError::Operation(format!("Invalid Sub-Agent tool name: {}", full_tool_name))
    })?;

    // Initialize tool trace
    let mut tool_trace = ToolCallTrace::new(
        tool_name.to_string(),
        subagent::SUBAGENT_TOOL_PREFIX.to_string(),
        tool_args.clone(),
    );

    // Get Sub-Agent context
    let ctx = subagent_ctx.ok_or_else(|| {
        let err_msg = "Sub-Agent tools require SubAgentToolContext";
        tool_trace.set_error(err_msg.to_string(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg.to_string())
    })?;

    let result = match tool_name {
        SPAWN_SUB_AGENT_TOOL => {
            let args: SpawnSubAgentArgs =
                serde_json::from_value(tool_args.clone()).map_err(|e| {
                    ServerError::Operation(format!("Invalid spawn_sub_agent args: {}", e))
                })?;

            dual_info!(
                "Spawning Sub-Agent '{}' - request_id: {}",
                args.name,
                request_id
            );

            subagent::execute_spawn_sub_agent(
                state.clone(),
                ctx.chat_server.clone(),
                ctx.headers.clone(),
                ctx.manager.clone(),
                ctx.available_tools.clone(),
                ctx.model.clone(),
                args.name,
                args.role,
                args.task,
                args.allowed_tools,
                args.wait_for_completion,
                args.timeout_secs,
                args.max_iterations,
                None, // parent_id
                emitter,
            )
            .await
        }
        GET_SUB_AGENT_RESULT_TOOL => {
            let args: GetSubAgentResultArgs =
                serde_json::from_value(tool_args.clone()).map_err(|e| {
                    ServerError::Operation(format!("Invalid get_sub_agent_result args: {}", e))
                })?;

            dual_info!(
                "Getting Sub-Agent result '{}' - request_id: {}",
                args.subagent_id,
                request_id
            );

            subagent::execute_get_sub_agent_result(
                ctx.manager.clone(),
                args.subagent_id,
                args.wait,
                args.timeout_secs,
            )
            .await
        }
        CANCEL_SUB_AGENT_TOOL => {
            let args: CancelSubAgentArgs =
                serde_json::from_value(tool_args.clone()).map_err(|e| {
                    ServerError::Operation(format!("Invalid cancel_sub_agent args: {}", e))
                })?;

            dual_info!(
                "Cancelling Sub-Agent '{}' - request_id: {}",
                args.subagent_id,
                request_id
            );

            subagent::execute_cancel_sub_agent(ctx.manager.clone(), args.subagent_id, args.reason)
                .await
        }
        _ => {
            let err_msg = format!("Unknown Sub-Agent tool: {}", tool_name);
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace);
            return Err(ServerError::Operation(err_msg));
        }
    };

    match result {
        Ok(value) => {
            let result_str =
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
            tool_trace.set_result(result_str.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace);
            Ok(result_str)
        }
        Err(e) => {
            let err_msg = e.to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace);
            Err(e)
        }
    }
}

/// Executes an internal tool (non-MCP tool).
///
/// # Supported Internal Tools
///
/// - `internal__skill_run_script`: Execute a script from the active skill
///   - Arguments: `script_name` (required), `args` (optional array)
///   - Requires an active skill to be loaded
///
/// - `internal__skill_load_asset`: Load an asset file from the active skill
///   - Arguments: `asset_name` (required), `variables` (optional object), `parse_as` (optional)
///   - Requires an active skill to be loaded
#[allow(clippy::too_many_arguments)]
async fn execute_internal_tool(
    state: &Arc<AppState>,
    full_tool_name: &str,
    tool_args: serde_json::Value,
    active_skills: &[LoadedSkill],
    conv_id: Option<&str>,
    request_id: &str,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let tool_name = parse_internal_tool_name(full_tool_name).ok_or_else(|| {
        ServerError::Operation(format!("Invalid internal tool name: {}", full_tool_name))
    })?;

    // Initialize tool trace for internal tool
    let mut tool_trace = ToolCallTrace::new(
        tool_name.to_string(),
        INTERNAL_TOOL_PREFIX.to_string(),
        tool_args.clone(),
    );

    match tool_name {
        SKILL_RUN_SCRIPT_TOOL => {
            execute_skill_run_script(
                tool_args,
                active_skills,
                conv_id,
                request_id,
                &mut tool_trace,
                iter_trace,
                start_time,
            )
            .await
        }
        SKILL_LOAD_ASSET_TOOL => {
            execute_skill_load_asset(
                tool_args,
                active_skills,
                request_id,
                &mut tool_trace,
                iter_trace,
                start_time,
            )
            .await
        }
        LANTAI_SEARCH_TOOL => {
            execute_lantai_search(state, tool_args, &mut tool_trace, iter_trace, start_time).await
        }
        LANTAI_STATS_TOOL => {
            execute_lantai_stats(state, &mut tool_trace, iter_trace, start_time).await
        }
        LANTAI_WRITE_MEMORY_TOOL => {
            execute_lantai_write_memory(state, tool_args, &mut tool_trace, iter_trace, start_time)
                .await
        }
        LANTAI_UPDATE_MEMORY_TOOL => {
            execute_lantai_update_memory(state, tool_args, &mut tool_trace, iter_trace, start_time)
                .await
        }
        LANTAI_DELETE_MEMORY_TOOL => {
            execute_lantai_delete_memory(state, tool_args, &mut tool_trace, iter_trace, start_time)
                .await
        }
        _ => {
            let err_msg = format!("Unknown internal tool: {}", tool_name);
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace);
            Err(ServerError::Operation(err_msg))
        }
    }
}

/// Executes the lantai_search internal tool.
async fn execute_lantai_search(
    state: &Arc<AppState>,
    tool_args: serde_json::Value,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let lantai_arc = state.lantai().ok_or_else(|| {
        let err_msg = "Lantai knowledge base is not initialized".to_string();
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let query = tool_args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_search requires 'query' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let limit = tool_args
        .get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(5);

    let guard = lantai_arc.lock().await;
    let search_query = lantai::SearchQuery::new(query, limit);
    let results = guard
        .search_with_options(&search_query)
        .await
        .map_err(|e| {
            let err_msg = format!("Lantai search failed: {e}");
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let output = if results.is_empty() {
        "No results found. IMPORTANT: This is a definitive result — the knowledge base does not contain relevant information. Do NOT retry the same search. Instead, try a different approach or provide your answer based on your own knowledge.".to_string()
    } else {
        let mut lines = Vec::new();
        for (i, r) in results.iter().enumerate() {
            lines.push(format!(
                "### Result {} (score: {:.4})\n**Source:** {}:{}-{}\n",
                i + 1,
                r.score,
                r.source_path,
                r.start_line,
                r.end_line,
            ));
            if !r.heading_path.is_empty() {
                lines.push(format!("**Path:** {}\n", r.heading_path));
            }
            lines.push(r.content.clone());
            lines.push(String::new());
        }
        lines.join("\n")
    };

    tool_trace.set_result(output.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(output)
}

/// Executes the lantai_stats internal tool.
async fn execute_lantai_stats(
    state: &Arc<AppState>,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let lantai_arc = state.lantai().ok_or_else(|| {
        let err_msg = "Lantai knowledge base is not initialized".to_string();
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let guard = lantai_arc.lock().await;
    let stats = guard.stats().map_err(|e| {
        let err_msg = format!("Lantai stats failed: {e}");
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let output = format!(
        "Knowledge Base Statistics:\n- Files: {}\n- Chunks: {}\n- Cached embeddings: {}",
        stats.total_files, stats.total_chunks, stats.total_cached_embeddings,
    );

    tool_trace.set_result(output.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(output)
}

/// Executes the lantai_write_memory internal tool.
async fn execute_lantai_write_memory(
    state: &Arc<AppState>,
    tool_args: serde_json::Value,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let writer = state.memory_writer().ok_or_else(|| {
        let err_msg = "MemoryWriter is not initialized".to_string();
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let content = tool_args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_write_memory requires 'content' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let category_str = tool_args
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("daily");
    let heading = tool_args.get("heading").and_then(|v| v.as_str());

    let category = match category_str {
        "daily" => lantai::writer::MemoryCategory::Daily,
        "core" => lantai::writer::MemoryCategory::Core,
        "experience" => lantai::writer::MemoryCategory::Experience,
        _ => {
            let err_msg = format!(
                "Invalid category: {category_str}. Must be 'daily', 'core', or 'experience'"
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            return Err(ServerError::Operation(err_msg));
        }
    };

    let request = lantai::writer::MemoryWriteRequest {
        content: content.to_string(),
        category,
        heading: heading.map(|s| s.to_string()),
    };

    let output = writer.write(&request).await.map_err(|e| {
        let err_msg = format!("Failed to write memory: {e}");
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    // Post-write compaction trigger for Core/Experience categories
    if matches!(
        category,
        lantai::writer::MemoryCategory::Core | lantai::writer::MemoryCategory::Experience
    ) {
        let config_guard = state.config.read().await;
        let compaction_enabled = config_guard
            .lantai
            .as_ref()
            .map(|l| l.auto_memory.compaction_enabled)
            .unwrap_or(true);
        let threshold = config_guard
            .lantai
            .as_ref()
            .map(|l| l.auto_memory.compaction_threshold)
            .unwrap_or(4000);
        let chat_info = config_guard.chat.clone();
        drop(config_guard);

        if compaction_enabled {
            let file_size = writer.file_size(category).await;
            if file_size > threshold as u64 {
                dual_info!(
                    "Memory compaction triggered: {} file size {} > threshold {}",
                    category_str,
                    file_size,
                    threshold
                );
                if let Some(chat_cfg) = chat_info {
                    let writer = writer.clone();
                    let chat_url =
                        format!("{}/chat/completions", chat_cfg.url.trim_end_matches('/'));
                    let api_key = chat_cfg.get_api_key();
                    let cat_label = category_str.to_string();
                    let model = chat_cfg.model.clone();
                    tokio::spawn(async move {
                        if let Err(e) = trigger_memory_compaction(
                            &writer,
                            category,
                            &chat_url,
                            api_key.as_deref(),
                            &model,
                        )
                        .await
                        {
                            tracing::warn!("Memory compaction failed for {}: {}", cat_label, e);
                        }
                    });
                }
            }
        }
    }

    tool_trace.set_result(output.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(output)
}

/// Executes the lantai_update_memory internal tool.
async fn execute_lantai_update_memory(
    state: &Arc<AppState>,
    tool_args: serde_json::Value,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let writer = state.memory_writer().ok_or_else(|| {
        let err_msg = "MemoryWriter is not initialized".to_string();
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let category_str = tool_args
        .get("category")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_update_memory requires 'category' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;
    let heading = tool_args
        .get("heading")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_update_memory requires 'heading' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;
    let content = tool_args
        .get("content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_update_memory requires 'content' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let category = match category_str {
        "core" => lantai::writer::MemoryCategory::Core,
        "experience" => lantai::writer::MemoryCategory::Experience,
        _ => {
            let err_msg = format!(
                "Invalid category for update: {category_str}. Must be 'core' or 'experience'"
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            return Err(ServerError::Operation(err_msg));
        }
    };

    writer
        .update_section(category, heading, content)
        .await
        .map_err(|e| {
            let err_msg = format!("Failed to update memory section: {e}");
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let output = format!("Updated section '{heading}' in {category_str}");
    tool_trace.set_result(output.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(output)
}

/// Executes the lantai_delete_memory internal tool.
async fn execute_lantai_delete_memory(
    state: &Arc<AppState>,
    tool_args: serde_json::Value,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    let writer = state.memory_writer().ok_or_else(|| {
        let err_msg = "MemoryWriter is not initialized".to_string();
        tool_trace.set_error(err_msg.clone(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg)
    })?;

    let category_str = tool_args
        .get("category")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_delete_memory requires 'category' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;
    let heading = tool_args
        .get("heading")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "lantai_delete_memory requires 'heading' argument".to_string();
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let category = match category_str {
        "core" => lantai::writer::MemoryCategory::Core,
        "experience" => lantai::writer::MemoryCategory::Experience,
        _ => {
            let err_msg = format!(
                "Invalid category for delete: {category_str}. Must be 'core' or 'experience'"
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            return Err(ServerError::Operation(err_msg));
        }
    };

    writer
        .delete_section(category, heading)
        .await
        .map_err(|e| {
            let err_msg = format!("Failed to delete memory section: {e}");
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    let output = format!("Deleted section '{heading}' from {category_str}");
    tool_trace.set_result(output.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(output)
}

/// Load memory context for system prompt injection.
///
/// Reads MEMORY.md (full) + today's daily log (last 5 entries) and concatenates
/// them into a context string, truncated to `max_chars`.
async fn load_memory_context(writer: &lantai::writer::MemoryWriter, max_chars: usize) -> String {
    let mut parts = Vec::new();

    // Core memory (highest priority)
    if let Ok(Some(core)) = writer.read_core_memory().await
        && !core.trim().is_empty()
    {
        parts.push(format!("### Core Memory\n{core}"));
    }

    // Today's daily log (last 5 bullet points)
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if let Ok(Some(daily)) = writer.read_daily_log(&today).await {
        let recent = truncate_to_last_n_lines(&daily, 5);
        if !recent.is_empty() {
            parts.push(format!("### Today's Recent Activity\n{recent}"));
        }
    }

    // Concatenate with priority-based truncation
    let mut result = String::new();
    for part in parts {
        if result.len() + part.len() > max_chars {
            let remaining = max_chars.saturating_sub(result.len());
            if remaining > 100 {
                result.push_str(&part[..remaining]);
                result.push_str("\n...(truncated)");
            }
            break;
        }
        if !result.is_empty() {
            result.push_str("\n\n");
        }
        result.push_str(&part);
    }
    result
}

/// Return the last N lines from content.
fn truncate_to_last_n_lines(content: &str, n: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= n {
        content.to_string()
    } else {
        lines[lines.len() - n..].join("\n")
    }
}

/// Extract a conversation snippet from messages for checkpoint summarization.
///
/// Collects user and assistant message content, truncated to `max_chars`.
fn extract_conversation_snippet(
    messages: &[ChatCompletionRequestMessage],
    max_chars: usize,
) -> String {
    let mut snippet = String::new();
    for msg in messages {
        let (role, text): (&str, &str) = match msg {
            ChatCompletionRequestMessage::User(u) => {
                let content_ref = u.content();
                if let ChatCompletionUserMessageContent::Text(t) = content_ref {
                    ("User", t.as_str())
                } else {
                    continue;
                }
            }
            ChatCompletionRequestMessage::Assistant(a) => {
                if let Some(c) = a.content() {
                    ("Assistant", c.as_str())
                } else {
                    continue;
                }
            }
            _ => continue,
        };
        if snippet.len() + text.len() + role.len() + 3 > max_chars {
            let remaining = max_chars.saturating_sub(snippet.len() + role.len() + 3);
            if remaining > 50 {
                // Truncate at char boundary
                let end = text
                    .char_indices()
                    .take_while(|(i, _)| *i < remaining)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(0);
                snippet.push_str(&format!("{role}: {}\n", &text[..end]));
            }
            break;
        }
        snippet.push_str(&format!("{role}: {text}\n"));
    }
    snippet
}

/// Fire-and-forget: record a DirectAnswer Q&A exchange to the daily log.
///
/// Checks config to see if auto_summary is enabled, then calls LLM to summarize the
/// user question + assistant answer and writes the result to today's daily log.
async fn trigger_direct_answer_memory(
    state: &AppState,
    chat_url: &str,
    api_key: Option<&str>,
    model: &str,
    user_message: &str,
    assistant_response: &str,
) {
    // Check if auto_summary is enabled in config
    let auto_summary_enabled = {
        let config = state.config.read().await;
        config
            .lantai
            .as_ref()
            .map(|l| l.auto_memory.auto_summary)
            .unwrap_or(true)
    };

    if !auto_summary_enabled {
        return;
    }

    let writer = match state.memory_writer() {
        Some(w) => w,
        None => return,
    };

    // Build a simple conversation snippet for the checkpoint
    let truncated = if assistant_response.len() > 2000 {
        let mut end = 2000;
        while !assistant_response.is_char_boundary(end) {
            end -= 1;
        }
        &assistant_response[..end]
    } else {
        assistant_response
    };
    let snippet = format!("User: {}\nAssistant: {}", user_message, truncated);

    if let Err(e) = trigger_memory_checkpoint(writer, chat_url, api_key, model, &snippet).await {
        tracing::warn!("Failed to record direct answer to memory: {}", e);
    }
}

/// Fire-and-forget: summarize a completed task plan and write to daily memory log.
async fn trigger_plan_memory(
    state: &AppState,
    chat_url: &str,
    api_key: Option<&str>,
    model: &str,
    user_message: &str,
    final_response: &str,
) {
    // Check if auto_summary is enabled in config
    let auto_summary_enabled = {
        let config = state.config.read().await;
        config
            .lantai
            .as_ref()
            .map(|l| l.auto_memory.auto_summary)
            .unwrap_or(true)
    };

    if !auto_summary_enabled {
        return;
    }

    let writer = match state.memory_writer() {
        Some(w) => w,
        None => return,
    };

    // Build a conversation snippet from user request + final synthesized response
    let truncated_response = if final_response.len() > 2000 {
        // Find a valid UTF-8 char boundary at or before byte 2000
        let mut end = 2000;
        while !final_response.is_char_boundary(end) {
            end -= 1;
        }
        &final_response[..end]
    } else {
        final_response
    };
    let snippet = format!(
        "User: {}\nAssistant (task plan result): {}",
        user_message, truncated_response
    );

    if let Err(e) = trigger_memory_checkpoint(writer, chat_url, api_key, model, &snippet).await {
        tracing::warn!("Failed to record plan result to memory: {}", e);
    }
}

/// Fire-and-forget: call LLM to extract key info from conversation, then write to daily log.
async fn trigger_memory_checkpoint(
    writer: &lantai::writer::MemoryWriter,
    chat_url: &str,
    api_key: Option<&str>,
    model: &str,
    conversation_snippet: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if conversation_snippet.trim().is_empty() {
        return Ok(());
    }

    let prompt = format!(
        "Summarize this conversation in ONE concise sentence. \
         Use the same language as the conversation. \
         Focus on the key topic and outcome. \
         Output ONLY the summary sentence, nothing else.\n\n\
         Conversation:\n{conversation_snippet}"
    );

    let request_json = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a concise note-taker. Summarize conversations in a single sentence using the same language as the conversation."},
            {"role": "user", "content": prompt}
        ],
        "stream": false
    });

    let mut req = reqwest::Client::new().post(chat_url);
    if let Some(key) = api_key {
        let auth = if key.starts_with("Bearer ") {
            key.to_string()
        } else {
            format!("Bearer {key}")
        };
        req = req.header("Authorization", auth);
    }
    req = req.header("Content-Type", "application/json");

    let resp = req.json(&request_json).send().await?;
    let body: serde_json::Value = resp.json().await?;

    let summary = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if summary.is_empty() {
        return Ok(());
    }

    // Write to daily log as a single-line entry
    let write_req = lantai::writer::MemoryWriteRequest {
        content: summary,
        category: lantai::writer::MemoryCategory::Daily,
        heading: None,
    };
    writer.write(&write_req).await.map_err(|e| {
        Box::new(std::io::Error::other(e.to_string())) as Box<dyn std::error::Error + Send + Sync>
    })?;

    tracing::info!("Memory checkpoint saved to daily log");
    Ok(())
}

/// Alias for subagent executor to call compaction.
pub(crate) async fn trigger_memory_compaction_bg(
    writer: &lantai::writer::MemoryWriter,
    category: lantai::writer::MemoryCategory,
    chat_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    trigger_memory_compaction(writer, category, chat_url, api_key, model).await
}

/// Auto-compact a memory file (MEMORY.md or EXPERIENCE.md) when it grows beyond threshold.
///
/// Reads the current file, asks LLM to consolidate/deduplicate, then rewrites atomically.
async fn trigger_memory_compaction(
    writer: &lantai::writer::MemoryWriter,
    category: lantai::writer::MemoryCategory,
    chat_url: &str,
    api_key: Option<&str>,
    model: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let current_content = match category {
        lantai::writer::MemoryCategory::Core => writer.read_core_memory().await,
        lantai::writer::MemoryCategory::Experience => writer.read_experience_memory().await,
        _ => return Ok(()),
    };

    let content = match current_content {
        Ok(Some(c)) if !c.trim().is_empty() => c,
        _ => return Ok(()),
    };

    let category_label = match category {
        lantai::writer::MemoryCategory::Core => "MEMORY.md (core preferences and decisions)",
        lantai::writer::MemoryCategory::Experience => {
            "EXPERIENCE.md (experience notes and patterns)"
        }
        _ => unreachable!(),
    };

    let prompt = format!(
        "You are compacting a knowledge base file: {category_label}.\n\
         The file uses ## headings to organize sections. Each section contains bullet-point entries.\n\n\
         Your task:\n\
         1. Merge duplicate or near-duplicate entries within each section.\n\
         2. Remove outdated entries that are superseded by newer ones.\n\
         3. Preserve all unique, valuable information.\n\
         4. Keep the same ## heading structure.\n\
         5. For Core memory: keep date prefixes (e.g., '- 2026-02-15: ...') on entries.\n\
         6. Output ONLY the compacted markdown, no explanation.\n\n\
         Current content:\n{content}"
    );

    let request_json = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a precise note compactor. Consolidate and deduplicate while preserving all unique information."},
            {"role": "user", "content": prompt}
        ],
        "stream": false
    });

    let mut req = reqwest::Client::new().post(chat_url);
    if let Some(key) = api_key {
        let auth = if key.starts_with("Bearer ") {
            key.to_string()
        } else {
            format!("Bearer {key}")
        };
        req = req.header("Authorization", auth);
    }
    req = req.header("Content-Type", "application/json");

    let resp = req.json(&request_json).send().await?;
    let body: serde_json::Value = resp.json().await?;

    let compacted = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if compacted.is_empty() || compacted.len() >= content.len() {
        // Compaction didn't reduce size; skip rewrite
        tracing::debug!(
            "Memory compaction produced no reduction ({} >= {}), skipping",
            compacted.len(),
            content.len()
        );
        return Ok(());
    }

    writer
        .rewrite_file(category, &compacted)
        .await
        .map_err(|e| {
            Box::new(std::io::Error::other(e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })?;

    tracing::info!(
        "Memory compaction complete: {} chars -> {} chars",
        content.len(),
        compacted.len()
    );
    Ok(())
}

/// Executes the skill_run_script internal tool.
///
/// # Arguments Schema
/// ```json
/// {
///   "script_name": "process.js",  // Required: name of the script file
///   "args": ["--input", "data.json"]  // Optional: command line arguments
/// }
/// ```
///
/// # Returns
/// The script output as a formatted string including stdout, stderr, and exit code.
async fn execute_skill_run_script(
    tool_args: serde_json::Value,
    active_skills: &[LoadedSkill],
    conv_id: Option<&str>,
    request_id: &str,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    // Require at least one active skill
    // For multi-skill scenarios, use the first skill that has the requested script
    let skill = active_skills.first().ok_or_else(|| {
        let err_msg =
            "skill_run_script requires an active skill. Use <use_skill> to activate a skill first.";
        tool_trace.set_error(err_msg.to_string(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg.to_string())
    })?;

    // Parse arguments
    let script_name = tool_args
        .get("script_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "skill_run_script requires 'script_name' argument";
            tool_trace.set_error(err_msg.to_string(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg.to_string())
        })?;

    let args: Vec<String> = tool_args
        .get("args")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    dual_info!(
        "🔧 Executing skill script: {} (skill: {}, args: {:?}) - request_id: {}",
        script_name,
        skill.metadata.name,
        args,
        request_id
    );

    // Build execution context
    let context =
        ScriptContext::with_ids(conv_id.map(|s| s.to_string()), Some(request_id.to_string()));

    // Execute the script
    match skill
        .execute_script_with_context(script_name, args.clone(), context, None)
        .await
    {
        Ok(output) => {
            // Format result for LLM
            let result = if output.exit_code == 0 {
                format!(
                    "Script '{}' executed successfully.\n\nOutput:\n{}",
                    script_name,
                    output.stdout.trim()
                )
            } else {
                format!(
                    "Script '{}' failed with exit code {}.\n\nStdout:\n{}\n\nStderr:\n{}\n\nIMPORTANT: If this failure is due to missing configuration (environment variables, API keys, credentials), missing dependencies, or permission issues, do NOT retry. Report the error to the user and suggest how to resolve it.",
                    script_name,
                    output.exit_code,
                    output.stdout.trim(),
                    output.stderr.trim()
                )
            };

            dual_info!(
                "✅ Script execution completed: {} (exit_code: {}, duration: {:?}) - request_id: {}",
                script_name,
                output.exit_code,
                output.duration,
                request_id
            );

            tool_trace.set_result(result.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            Ok(result)
        }
        Err(e) => {
            let err_msg = format!("Script execution failed: {}", e);
            dual_warn!(
                "❌ Script execution failed: {} - {} - request_id: {}",
                script_name,
                e,
                request_id
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            Err(ServerError::Operation(err_msg))
        }
    }
}

/// Executes the skill_load_asset internal tool.
///
/// # Arguments Schema
/// ```json
/// {
///   "asset_name": "template.md",  // Required: name of the asset file
///   "variables": {                 // Optional: variables for template replacement
///     "name": "John",
///     "date": "2024-01-15"
///   },
///   "parse_as": "json"            // Optional: parse content as "json", "yaml", or "markdown"
/// }
/// ```
///
/// # Returns
/// The asset content as a formatted string, optionally with variables replaced
/// and/or parsed as structured data.
async fn execute_skill_load_asset(
    tool_args: serde_json::Value,
    active_skills: &[LoadedSkill],
    request_id: &str,
    tool_trace: &mut ToolCallTrace,
    iter_trace: &mut IterationTrace,
    start_time: Instant,
) -> ServerResult<String> {
    // Require at least one active skill
    // For multi-skill scenarios, use the first skill that has the requested asset
    let skill = active_skills.first().ok_or_else(|| {
        let err_msg =
            "skill_load_asset requires an active skill. Use <use_skill> to activate a skill first.";
        tool_trace.set_error(err_msg.to_string(), start_time.elapsed());
        iter_trace.add_tool_call(tool_trace.clone());
        ServerError::Operation(err_msg.to_string())
    })?;

    // Parse arguments
    let asset_name = tool_args
        .get("asset_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            let err_msg = "skill_load_asset requires 'asset_name' argument";
            tool_trace.set_error(err_msg.to_string(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg.to_string())
        })?;

    let variables = tool_args
        .get("variables")
        .and_then(|v| v.as_object())
        .cloned();

    let parse_as = tool_args
        .get("parse_as")
        .and_then(|v| v.as_str())
        .map(|s| s.to_lowercase());

    dual_info!(
        "📦 Loading skill asset: {} (skill: {}, parse_as: {:?}) - request_id: {}",
        asset_name,
        skill.metadata.name,
        parse_as,
        request_id
    );

    // Load asset content
    let content = SkillLoader::load_asset_string(&skill.skill_dir, asset_name)
        .await
        .ok_or_else(|| {
            let err_msg = format!(
                "Asset '{}' not found in skill '{}'",
                asset_name, skill.metadata.name
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            ServerError::Operation(err_msg)
        })?;

    // Apply template variable replacement if variables provided
    let content = if let Some(vars) = variables {
        apply_template_variables(&content, &vars)
    } else {
        content
    };

    // Parse content if requested
    let result = match parse_as.as_deref() {
        Some("json") => parse_as_json(&content, asset_name)?,
        Some("yaml") => parse_as_yaml(&content, asset_name)?,
        Some("markdown") | Some("md") => format_as_markdown(&content, asset_name),
        Some(unknown) => {
            let err_msg = format!(
                "Unknown parse_as format '{}'. Supported: json, yaml, markdown",
                unknown
            );
            tool_trace.set_error(err_msg.clone(), start_time.elapsed());
            iter_trace.add_tool_call(tool_trace.clone());
            return Err(ServerError::Operation(err_msg));
        }
        None => format!(
            "Asset '{}' loaded successfully.\n\nContent:\n{}",
            asset_name, content
        ),
    };

    dual_info!(
        "✅ Asset loaded: {} (size: {} bytes) - request_id: {}",
        asset_name,
        result.len(),
        request_id
    );

    tool_trace.set_result(result.clone(), start_time.elapsed());
    iter_trace.add_tool_call(tool_trace.clone());
    Ok(result)
}

/// Apply template variable replacement using {{variable}} syntax.
///
/// Replaces occurrences of `{{variable_name}}` with the corresponding value
/// from the variables map. Variables that are not found remain unchanged.
fn apply_template_variables(
    content: &str,
    variables: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut result = content.to_string();

    for (key, value) in variables {
        let placeholder = format!("{{{{{}}}}}", key); // {{key}}
        let replacement = match value {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::Bool(b) => b.to_string(),
            serde_json::Value::Null => "null".to_string(),
            _ => value.to_string(), // For arrays/objects, use JSON representation
        };
        result = result.replace(&placeholder, &replacement);
    }

    result
}

/// Parse content as JSON and format for display.
fn parse_as_json(content: &str, asset_name: &str) -> ServerResult<String> {
    let parsed: serde_json::Value = serde_json::from_str(content).map_err(|e| {
        ServerError::Operation(format!("Failed to parse '{}' as JSON: {}", asset_name, e))
    })?;

    let formatted = serde_json::to_string_pretty(&parsed).map_err(|e| {
        ServerError::Operation(format!(
            "Failed to format JSON from '{}': {}",
            asset_name, e
        ))
    })?;

    Ok(format!(
        "Asset '{}' loaded and parsed as JSON.\n\nContent:\n```json\n{}\n```",
        asset_name, formatted
    ))
}

/// Parse content as YAML and format for display.
fn parse_as_yaml(content: &str, asset_name: &str) -> ServerResult<String> {
    // Parse YAML to validate it
    let parsed: serde_yaml::Value = serde_yaml::from_str(content).map_err(|e| {
        ServerError::Operation(format!("Failed to parse '{}' as YAML: {}", asset_name, e))
    })?;

    // Re-serialize for consistent formatting
    let formatted = serde_yaml::to_string(&parsed).map_err(|e| {
        ServerError::Operation(format!(
            "Failed to format YAML from '{}': {}",
            asset_name, e
        ))
    })?;

    Ok(format!(
        "Asset '{}' loaded and parsed as YAML.\n\nContent:\n```yaml\n{}\n```",
        asset_name, formatted
    ))
}

/// Format content as Markdown for display.
fn format_as_markdown(content: &str, asset_name: &str) -> String {
    format!(
        "Asset '{}' loaded as Markdown.\n\nContent:\n\n{}",
        asset_name, content
    )
}

/// Determines if an error is retryable for subtask execution.
///
/// Retryable errors include:
/// - Tool call failures (including retry exhausted)
/// - Maximum iterations exceeded
/// - Timeout errors
/// - MCP operation errors
///
/// Non-retryable errors include:
/// - Client cancellation
/// - Configuration errors
/// - Parse errors
fn is_retryable_error(error: &ServerError) -> bool {
    matches!(
        error,
        ServerError::ToolCallRetryExhausted { .. }
            | ServerError::MaxIterationsExceeded(_)
            | ServerError::SubtaskTimeout { .. }
            | ServerError::McpOperation(_)
            | ServerError::McpEmptyContent
    )
}

/// Filters tools based on skills' allowed_tools.
///
/// - Phase 1 (no active_skills): Filters out tools that are covered by any skill's allowed_tools
/// - Phase 2 (with active_skills): Only shows tools declared in the skills' merged allowed_tools (if any)
///
/// This prevents redundancy between skill descriptions and tool listings.
fn filter_tools_by_skills<'a>(
    tools: &'a [ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    active_skills: &[LoadedSkill],
) -> Vec<&'a ToolDescription> {
    if !active_skills.is_empty() {
        // Phase 2: Only show skill-related tools (merged from all active skills)
        let merged_tools = SkillInjector::merge_allowed_tools(active_skills);
        if merged_tools.is_empty() {
            // No allowed-tools specified in any skill, show all tools (backward compatibility)
            tools.iter().collect()
        } else {
            // Only show tools declared in merged allowed-tools
            let skill_tools_set: HashSet<&str> = merged_tools.iter().map(|s| s.as_str()).collect();

            let filtered: Vec<&ToolDescription> = tools
                .iter()
                .filter(|t| skill_tools_set.contains(t.name.as_str()))
                .collect();

            if active_skills.len() == 1 {
                dual_debug!(
                    "Phase 2 tool filtering: skill '{}' allows {} tools, showing {} of {} available",
                    active_skills[0].metadata.name,
                    merged_tools.len(),
                    filtered.len(),
                    tools.len()
                );
            } else {
                let skill_names: Vec<&str> = active_skills
                    .iter()
                    .map(|s| s.metadata.name.as_str())
                    .collect();
                dual_debug!(
                    "Phase 2 tool filtering: {} skills [{}] allow {} merged tools, showing {} of {} available",
                    active_skills.len(),
                    skill_names.join(", "),
                    merged_tools.len(),
                    filtered.len(),
                    tools.len()
                );
            }

            filtered
        }
    } else {
        // Phase 1: Filter out tools covered by skills
        // Collect all tools covered by any skill
        let covered_tools: HashSet<&str> = skills_summaries
            .map(|summaries| {
                summaries
                    .iter()
                    .flat_map(|s| s.allowed_tools.iter().map(|t| t.as_str()))
                    .collect()
            })
            .unwrap_or_default();

        if covered_tools.is_empty() {
            // No tools to filter, show all
            return tools.iter().collect();
        }

        // Filter out covered tools
        let filtered: Vec<&ToolDescription> = tools
            .iter()
            .filter(|t| !covered_tools.contains(t.name.as_str()))
            .collect();

        dual_debug!(
            "Phase 1 tool filtering: {} tools covered by skills, showing {} of {} available",
            covered_tools.len(),
            filtered.len(),
            tools.len()
        );

        filtered
    }
}

/// Builds the initial context messages for React loop execution.
///
/// This function supports two-phase skill loading:
/// - Phase 1 (no active_skills): Injects skills summaries, allows LLM to request skills
/// - Phase 2 (with active_skills): Injects full skill content, references, and filtered tools
///
/// Supports multi-skill activation: when multiple skills are active, their content
/// is merged using `SkillInjector::multi_skill_injection_auto_refs()`.
///
/// # Arguments
/// * `subtask` - The current subtask being executed
/// * `previous_results` - Results from dependent subtasks
/// * `available_tools` - List of available MCP tools
/// * `skills_summaries` - Optional skill summaries for Phase 1
/// * `active_skills` - Active skills for Phase 2 (empty slice for Phase 1)
/// * `max_reference_size` - Maximum total size of reference documents to load (0 = no limit)
pub(crate) async fn build_context_for_react(
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    available_tools: &[ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    active_skills: &[LoadedSkill],
    max_reference_size: usize,
    file_attachments: &[FileAttachmentInfo],
) -> Vec<ChatCompletionRequestMessage> {
    let mut messages = Vec::new();

    // Filter tools based on skills' allowed_tools
    let filtered_tools = filter_tools_by_skills(available_tools, skills_summaries, active_skills);

    // Build tools description from filtered list
    let tools_desc = if filtered_tools.is_empty() {
        "No tools are currently available.".to_string()
    } else {
        filtered_tools
            .iter()
            .map(|t| {
                if let Some(params) = &t.parameters {
                    format!(
                        "- {}: {}\n  Parameters: {}",
                        t.name,
                        t.description,
                        serde_json::to_string(params).unwrap_or_default()
                    )
                } else {
                    format!("- {}: {}", t.name, t.description)
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Build the system prompt based on whether we have active skills
    let system_prompt = if !active_skills.is_empty() {
        // Phase 2: Active skills - inject full skill content with references
        // For multi-skill, use multi_skill_injection_auto_refs to merge all skills
        let skill_section =
            SkillInjector::multi_skill_injection_auto_refs(active_skills, max_reference_size).await;

        // Script notes are only needed when skills have scripts
        // Note: Script call examples are already generated by SkillInjector::generate_scripts_section
        // inside skill_section, so we only add usage notes here to avoid duplicate examples.
        let has_scripts = active_skills.iter().any(|s| !s.scripts.is_empty());
        let script_notes = if has_scripts {
            r#"- `internal__skill_run_script`:
  - `script_name`: Just the filename (e.g., "convert.py"), not the full path
  - `args`: Array of command line arguments to pass to the script
"#
        } else {
            ""
        };

        format!(
            r#"You are an AI assistant executing a specific subtask as part of a larger plan.

## Your Task
{}

{}

## Available Tools
{}

## Instructions
1. Follow the skill instructions above to complete the task
2. Use the available tools as needed
3. When you have completed the task, provide your final answer wrapped in <final_answer></final_answer> tags
4. If a script fails with a configuration or environment error (missing environment variables, API keys, dependencies, or permissions), do NOT retry the same script. Instead, report the error to the user in your final answer and explain what needs to be fixed.
5. Only retry a script if you can meaningfully change the arguments or approach.

## Response Format
- Use <thought></thought> tags to explain your reasoning
- Use <action></action> tags for tool calls with JSON format:
  <action>{{"name": "tool_name", "arguments": {{"param": "value"}}}}</action>
- When done, use <final_answer></final_answer> tags for your final response

## Tool Call Examples

### Example 1: MCP Tool Call
<thought>I need to calculate the sum of two numbers</thought>
<action>{{"name": "mcp__cardea-calculator__sum", "arguments": {{"a": 23, "b": 32}}}}</action>

### Example 2: Load an Asset with Template Variables
<thought>I need to load a template and fill in the variables</thought>
<action>{{"name": "internal__skill_load_asset", "arguments": {{"asset_name": "report-template.md", "variables": {{"title": "Monthly Report", "date": "2024-01-15"}}}}}}</action>

### Example 3: Load and Parse a JSON Configuration
<thought>I need to read the configuration file as structured data</thought>
<action>{{"name": "internal__skill_load_asset", "arguments": {{"asset_name": "config.json", "parse_as": "json"}}}}</action>

### Example 4: Spawn a Sub-Agent for Parallel Task Execution
<thought>I need to perform multiple independent searches in parallel. I'll spawn Sub-Agents for each search.</thought>
<action>{{"name": "internal__spawn_sub_agent", "arguments": {{"name": "WebSearcher", "role": "You are a research assistant specialized in web searching.", "task": "Search for the latest news about AI developments in 2024", "wait_for_completion": false}}}}</action>

### Example 5: Get Sub-Agent Result
<thought>I need to check the result from the Sub-Agent I spawned earlier.</thought>
<action>{{"name": "internal__get_sub_agent_result", "arguments": {{"subagent_id": "subagent_abc123", "wait": true, "timeout_secs": 60}}}}</action>

### Example 6: Cancel a Sub-Agent
<thought>The Sub-Agent is taking too long, I need to cancel it.</thought>
<action>{{"name": "internal__cancel_sub_agent", "arguments": {{"subagent_id": "subagent_abc123", "reason": "Task no longer needed"}}}}</action>

**Important**: When using internal tools:
{script_notes}- `internal__skill_load_asset`:
  - `asset_name`: Just the filename in the assets/ directory
  - `variables`: Object with key-value pairs to replace {{key}} in the template
  - `parse_as`: Optional format ("json", "yaml", "markdown") for structured parsing
- `internal__spawn_sub_agent` (for parallel task execution):
  - `name`: A descriptive name for the Sub-Agent (e.g., "DataAnalyst", "WebSearcher")
  - `role`: System prompt describing the Sub-Agent's expertise and behavior
  - `task`: The specific task for the Sub-Agent to complete
  - `wait_for_completion`: Set to `true` to wait for result, `false` for async execution
  - `allowed_tools`: Optional list of tools the Sub-Agent can use
  - `timeout_secs`: Optional timeout in seconds
- `internal__get_sub_agent_result`:
  - `subagent_id`: The ID returned from spawn_sub_agent
  - `wait`: Set to `true` to wait for completion if still running
  - `timeout_secs`: Optional wait timeout in seconds
- `internal__cancel_sub_agent`:
  - `subagent_id`: The ID of the Sub-Agent to cancel
  - `reason`: Optional reason for cancellation

**When to use Sub-Agents**:
- Use Sub-Agents when you need to perform multiple independent tasks in parallel
- Examples: searching multiple sources simultaneously, analyzing different data sets
- Spawn multiple Sub-Agents with `wait_for_completion: false`, then collect results with `get_sub_agent_result`
- Each Sub-Agent runs independently with its own context and tools

After receiving the observation, provide your final answer:
<thought>I received the result</thought>
<final_answer>The task is complete.</final_answer>

Remember: Focus only on this specific subtask. Follow the skill instructions carefully."#,
            subtask.description,
            skill_section,
            tools_desc,
            script_notes = script_notes,
        )
    } else {
        // Phase 1: No active skills - show skills summaries if available using SkillInjector
        // Build skills section using SkillInjector
        let skills_section = match skills_summaries {
            Some(summaries) => SkillInjector::phase1_injection(summaries),
            None => String::new(),
        };
        let has_skills = !skills_section.is_empty();

        let skill_important_note = if has_skills {
            r#"
**⚠️ IMPORTANT**: Tools listed above can be called DIRECTLY using <action> tags. Do NOT use <use_skill> tags for these tools. The <use_skill> tag is ONLY for loading skills from the "Available Skills" section.
"#
        } else {
            ""
        };

        let skill_instruction = if has_skills {
            r#"
3. Only if a **skill** (from "Available Skills" section, NOT "Available Tools") would help, request it using <use_skill>skill-name</use_skill> tags
   - You can request multiple skills: <use_skill>skill-a, skill-b</use_skill>
   - **⚠️ IMPORTANT**: When you request a skill, ONLY output the <use_skill> tag. Do NOT include any <action> tags in the same response. The system will load the skill and provide the actual tool list in the next turn."#
        } else {
            ""
        };

        let skill_example = if has_skills {
            r#"
### Request a Skill (Only for items in "Available Skills")
<thought>I need to perform a calculation. The cardea-calculator skill can help with this.</thought>
<use_skill>cardea-calculator</use_skill>

(Do NOT add <action> tags here. Wait for the skill to be loaded in the next turn.)
"#
        } else {
            ""
        };

        format!(
            r#"You are an AI assistant executing a specific subtask as part of a larger plan.

## Your Task
{}
{}
## Available Tools
{}
{}
## Instructions
1. Analyze the task and think about how to accomplish it
2. To call a tool from "Available Tools", use <action> tags directly:
   <action>{{"name": "tool_name", "arguments": {{"param": "value"}}}}
</action>{}
3. When you have completed the task, provide your final answer wrapped in <final_answer></final_answer> tags

## Response Format
- Use <thought></thought> tags to explain your reasoning
- Use <action></action> tags for tool calls with JSON format:
  <action>{{"name": "tool_name", "arguments": {{"param": "value"}}}}</action>
- When done, use <final_answer></final_answer> tags for your final response

## Tool Call Examples

### Direct Tool Call
<thought>I need to convert an address to coordinates using the geo tool.</thought>
<action>{{"name": "mcp__amap__maps_geo", "arguments": {{"address": "北京市", "city": "北京"}}}}</action>
{}
### Spawn a Sub-Agent for Parallel Task Execution
<thought>I need to perform multiple independent searches in parallel. I'll spawn Sub-Agents for each search.</thought>
<action>{{"name": "internal__spawn_sub_agent", "arguments": {{"name": "WebSearcher", "role": "You are a research assistant specialized in web searching.", "task": "Search for the latest news about AI developments", "wait_for_completion": false}}}}</action>

### Get Sub-Agent Result
<thought>I need to check the result from the Sub-Agent I spawned earlier.</thought>
<action>{{"name": "internal__get_sub_agent_result", "arguments": {{"subagent_id": "subagent_abc123", "wait": true}}}}</action>

**When to use Sub-Agents**:
- Use Sub-Agents when you need to perform multiple independent tasks in parallel
- Examples: searching multiple sources simultaneously, analyzing different data sets
- Spawn multiple Sub-Agents with `wait_for_completion: false`, then collect results with `get_sub_agent_result`

Remember: Focus only on this specific subtask. Use the context from previous results if needed."#,
            subtask.description,
            skills_section,
            tools_desc,
            skill_important_note,
            skill_instruction,
            skill_example,
        )
    };

    messages.push(ChatCompletionRequestMessage::System(
        ChatCompletionSystemMessage::new(system_prompt, None),
    ));

    // Add context from previous results if there are dependencies
    if !subtask.dependencies.is_empty() {
        let context_parts: Vec<String> = subtask
            .dependencies
            .iter()
            .filter_map(|dep_id| {
                previous_results
                    .iter()
                    .find(|(id, _)| id == dep_id)
                    .map(|(id, result)| {
                        format!(
                            "**任务{}的结果** = {}\n(当任务描述中提到\"任务{}的结果\"时，直接使用上面的值)",
                            id, result, id
                        )
                    })
            })
            .collect();

        if !context_parts.is_empty() {
            let context_message = format!(
                "## ⚠️ 重要：前置任务结果\n\n\
                以下是本任务依赖的前置任务的执行结果。**你必须直接使用这些结果值**，不要重新计算或重新执行已完成的操作。\n\n\
                {}\n\n\
                **说明**：如果任务描述中包含\"任务N的结果\"这样的引用，请将其替换为上面对应的实际值。",
                context_parts.join("\n\n")
            );
            messages.push(ChatCompletionRequestMessage::User(
                ChatCompletionUserMessage::new(
                    ChatCompletionUserMessageContent::Text(context_message),
                    None,
                ),
            ));
        }
    }

    // Add the task prompt
    let task_prompt = format!(
        "Please complete the following task: {}",
        subtask.description
    );
    messages.push(ChatCompletionRequestMessage::User(
        ChatCompletionUserMessage::new(ChatCompletionUserMessageContent::Text(task_prompt), None),
    ));

    // Inject file attachment content (if any)
    if !file_attachments.is_empty() {
        let mut file_parts: Vec<endpoints::chat::ContentPart> = Vec::new();
        file_parts.push(endpoints::chat::ContentPart::Text(
            endpoints::chat::TextContentPart::new("[User attached files]"),
        ));
        for file in file_attachments {
            file_parts.extend(resolve_file_for_llm(file));
        }
        messages.push(ChatCompletionRequestMessage::User(
            ChatCompletionUserMessage::new(
                ChatCompletionUserMessageContent::Parts(file_parts),
                None,
            ),
        ));
    }

    messages
}

/// Filters tools based on allowed patterns from a Skill's allowed-tools field.
///
/// Pattern formats supported:
/// - Exact match: "tool-name" matches "tool-name---server"
/// - Wildcard: "Bash(git:*)" matches "Bash(git:status)---server", "Bash(git:commit)---server"
/// - Simple wildcard: "Bash*" matches any tool starting with "Bash"
///
/// If allowed_patterns is None or empty, all tools are allowed.
fn filter_tools_by_patterns<'a>(
    tools: &'a [ToolDescription],
    allowed_patterns: Option<&[String]>,
) -> Vec<&'a ToolDescription> {
    match allowed_patterns {
        None | Some([]) => tools.iter().collect(),
        Some(patterns) => tools
            .iter()
            .filter(|tool| {
                // Extract the short tool name part from MCP tool name
                let short_name = extract_tool_name(&tool.name);
                // Also keep the full name for matching patterns that use full MCP names
                // (e.g., "mcp__cardea-weather__get_current_weather")
                let full_name = &tool.name;

                patterns.iter().any(|pattern| {
                    if pattern.contains('*') {
                        // Wildcard pattern matching (against short name)
                        match_wildcard_pattern(pattern, short_name)
                    } else {
                        // Exact match (case-insensitive) against both full and short name
                        short_name.eq_ignore_ascii_case(pattern)
                            || full_name.eq_ignore_ascii_case(pattern)
                    }
                })
            })
            .collect(),
    }
}

/// Matches a tool name against a wildcard pattern.
///
/// Supports:
/// - "*" at the end: "Bash*" matches "Bash", "Bash(git:status)", etc.
/// - "*" within parentheses: "Bash(git:*)" matches "Bash(git:status)", "Bash(git:commit)"
fn match_wildcard_pattern(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" {
        return true;
    }

    // Handle "prefix*" pattern
    if pattern.ends_with('*') && !pattern.contains('(') {
        let prefix = &pattern[..pattern.len() - 1];
        return tool_name.starts_with(prefix);
    }

    // Handle "Name(prefix:*)" pattern - e.g., "Bash(git:*)"
    if let Some(star_pos) = pattern.find('*') {
        let pattern_prefix = &pattern[..star_pos];
        let pattern_suffix = &pattern[star_pos + 1..];

        // Tool name must start with pattern_prefix and end with pattern_suffix
        if tool_name.starts_with(pattern_prefix) && tool_name.ends_with(pattern_suffix) {
            return true;
        }
    }

    false
}

/// Builds the tools JSON for the LLM request.
///
/// If `allowed_patterns` is provided, only tools matching the patterns are included.
/// Internal tools (like `skill_run_script`, `skill_load_asset`) get custom parameter schemas.
fn build_tools_json(
    available_tools: &[ToolDescription],
    allowed_patterns: Option<&[String]>,
) -> serde_json::Value {
    let filtered_tools = filter_tools_by_patterns(available_tools, allowed_patterns);

    let tools: Vec<serde_json::Value> = filtered_tools
        .iter()
        .map(|tool| {
            // Check if this is an internal tool with custom schema
            let is_skill_run_script = tool.name == internal_tool_name(SKILL_RUN_SCRIPT_TOOL);
            let is_skill_load_asset = tool.name == internal_tool_name(SKILL_LOAD_ASSET_TOOL);

            let parameters = if is_skill_run_script {
                // Custom schema for skill_run_script
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "script_name": {
                            "type": "string",
                            "description": "Name of the script file to execute (e.g., 'process.js', 'export.py')"
                        },
                        "args": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional command line arguments to pass to the script"
                        }
                    },
                    "required": ["script_name"]
                })
            } else if is_skill_load_asset {
                // Custom schema for skill_load_asset
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "asset_name": {
                            "type": "string",
                            "description": "Name of the asset file to load (e.g., 'template.md', 'config.json')"
                        },
                        "variables": {
                            "type": "object",
                            "description": "Optional variables for template replacement. Use {{variable}} syntax in the template."
                        },
                        "parse_as": {
                            "type": "string",
                            "enum": ["json", "yaml", "markdown"],
                            "description": "Optional format to parse the content as. If not specified, returns raw content."
                        }
                    },
                    "required": ["asset_name"]
                })
            } else if let Some(ref params) = tool.parameters {
                // Use actual tool parameters from MCP schema
                params.clone()
            } else {
                // Fallback for tools without schema
                serde_json::json!({
                    "type": "object",
                    "properties": {},
                    "required": []
                })
            };

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

    serde_json::Value::Array(tools)
}

/// Generates the final response by asking LLM to summarize results.
async fn generate_final_response(
    _state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    headers: &HeaderMap,
    original_request: &ChatCompletionRequest,
    plan: &TaskPlan,
    results: &[(usize, String)],
    request_id: &str,
) -> ServerResult<ChatCompletionObject> {
    // Build summary prompt
    let results_summary: String = results
        .iter()
        .map(|(id, result)| {
            let subtask = plan.subtasks.iter().find(|s| s.id == *id);
            let desc = subtask
                .map(|s| s.description.as_str())
                .unwrap_or("Unknown task");
            format!("- Task {}: {}\n  Result: {}", id, desc, result)
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let summary_prompt = format!(
        "Based on the following task execution results, provide a comprehensive answer to the user's original request.\n\n\
        Original Goal: {}\n\n\
        Execution Results:\n{}\n\n\
        Please synthesize these results into a clear, coherent response.",
        plan.original_goal, results_summary
    );

    // Create summary request (build new request instead of cloning)
    let summary_messages = vec![endpoints::chat::ChatCompletionRequestMessage::User(
        endpoints::chat::ChatCompletionUserMessage::new(
            endpoints::chat::ChatCompletionUserMessageContent::Text(summary_prompt),
            None,
        ),
    )];

    let model_name = original_request.model.as_deref().unwrap_or("default");

    let summary_request = serde_json::json!({
        "model": model_name,
        "messages": summary_messages,
        "stream": false
    });

    // Build request
    let url = format!("{}/chat/completions", chat_server.url.trim_end_matches('/'));
    let mut client = reqwest::Client::new().post(&url);
    client = client.header(CONTENT_TYPE, "application/json");

    if let Some(api_key) = &chat_server.api_key {
        let auth = if api_key.starts_with("Bearer ") {
            api_key.clone()
        } else {
            format!("Bearer {}", api_key)
        };
        client = client.header(AUTHORIZATION, auth);
    } else if let Some(auth) = headers.get("authorization")
        && let Ok(auth_str) = auth.to_str()
    {
        client = client.header(AUTHORIZATION, auth_str);
    }

    dual_debug!(
        "Sending summary request to LLM - request_id: {}",
        request_id
    );

    // Send request
    let response =
        client.json(&summary_request).send().await.map_err(|e| {
            ServerError::Operation(format!("Failed to send summary request: {}", e))
        })?;

    let chat_completion: ChatCompletionObject = response
        .json()
        .await
        .map_err(|e| ServerError::Operation(format!("Failed to parse summary response: {}", e)))?;

    Ok(chat_completion)
}

/// Builds the final HTTP response.
///
/// When `enhanced_stream_config.enabled` is true, the response will use
/// custom SSE event types (thought, tool_call, tool_result, text, status, finish).
/// Otherwise, it uses standard OpenAI-compatible streaming format.
fn build_response(
    mut chat_completion: ChatCompletionObject,
    final_content: &str,
    stream: bool,
    enhanced_stream_config: &EnhancedStreamConfig,
    event_receiver: Option<mpsc::Receiver<String>>,
    request_id: &str,
) -> ServerResult<Response<Body>> {
    if stream {
        if enhanced_stream_config.enabled {
            // Enhanced streaming mode: combine pre-collected events with text chunks
            build_enhanced_streaming_response(final_content, event_receiver, request_id)
        } else {
            // Standard OpenAI-compatible streaming mode
            build_standard_streaming_response(chat_completion, final_content, request_id)
        }
    } else {
        // Non-streaming response
        chat_completion.choices[0].message.content = Some(final_content.to_string());
        let response_body = serde_json::to_string(&chat_completion)
            .map_err(|e| ServerError::Operation(format!("Failed to serialize response: {}", e)))?;

        Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .status(StatusCode::OK)
            .body(Body::from(response_body))
            .map_err(|e| {
                dual_error!(
                    "Failed to create response: {} - request_id: {}",
                    e,
                    request_id
                );
                ServerError::Operation(format!("Failed to create response: {}", e))
            })
    }
}

/// Builds enhanced streaming response with custom SSE event types.
fn build_enhanced_streaming_response(
    final_content: &str,
    event_receiver: Option<mpsc::Receiver<String>>,
    request_id: &str,
) -> ServerResult<Response<Body>> {
    use super::events::{TextEvent, format_sse_event};

    // Collect all pre-emitted events from the receiver
    let mut pre_events: Vec<String> = Vec::new();
    if let Some(mut receiver) = event_receiver {
        // Try to receive all pending events (non-blocking)
        while let Ok(event) = receiver.try_recv() {
            pre_events.push(event);
        }
    }

    // Generate text chunks for the final content
    let text_chunks = gen_chunks_with_formatting(final_content, 10);
    let text_events: Vec<String> = text_chunks
        .into_iter()
        .map(|chunk| format_sse_event("text", &TextEvent { content: chunk }))
        .collect();

    // Combine pre-events, text events, and done marker
    let all_events: Vec<String> = pre_events
        .into_iter()
        .chain(text_events)
        .chain(std::iter::once("data: [DONE]\n\n".to_string()))
        .collect();

    let request_id_owned = request_id.to_string();
    let stream =
        stream::iter(all_events).map(|s| Ok::<_, std::convert::Infallible>(s.into_bytes()));

    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Enhanced-Stream", "true")
        .status(StatusCode::OK)
        .body(Body::from_stream(stream))
        .map_err(|e| {
            dual_error!(
                "Failed to create enhanced streaming response: {} - request_id: {}",
                e,
                request_id_owned
            );
            ServerError::Operation(format!(
                "Failed to create enhanced streaming response: {}",
                e
            ))
        })
}

/// Builds standard OpenAI-compatible streaming response.
fn build_standard_streaming_response(
    chat_completion: ChatCompletionObject,
    final_content: &str,
    request_id: &str,
) -> ServerResult<Response<Body>> {
    let chunks = gen_chunks_with_formatting(final_content, 10);
    let id = gen_chat_id();
    let model = chat_completion.model.clone();
    let usage = chat_completion.usage;
    let chunks_len = chunks.len();

    let request_id_owned = request_id.to_string();
    let stream = stream::iter(chunks.into_iter().enumerate().map(move |(i, chunk)| {
        let created = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let mut chat_completion_chunk = ChatCompletionChunk {
            id: id.clone(),
            object: "chat.completion.chunk".to_string(),
            created,
            model: model.clone(),
            system_fingerprint: "fp_plan_mode".to_string(),
            choices: vec![ChatCompletionChunkChoice {
                index: i as u32,
                delta: ChatCompletionChunkChoiceDelta {
                    role: ChatCompletionRole::Assistant,
                    content: Some(chunk),
                    tool_calls: vec![],
                },
                logprobs: None,
                finish_reason: None,
            }],
            usage: None,
        };

        if i == chunks_len - 1 {
            chat_completion_chunk.choices[0].finish_reason =
                Some(endpoints::common::FinishReason::stop);
            chat_completion_chunk.usage = Some(usage);
        }

        let json_str = serde_json::to_string(&chat_completion_chunk).unwrap();
        format!("data: {json_str}\n\n")
    }))
    .chain(stream::once(async { "data: [DONE]\n\n".to_string() }))
    .map(|s| Ok::<_, std::convert::Infallible>(s.into_bytes()));

    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .status(StatusCode::OK)
        .body(Body::from_stream(stream))
        .map_err(|e| {
            dual_error!(
                "Failed to create streaming response: {} - request_id: {}",
                e,
                request_id_owned
            );
            ServerError::Operation(format!("Failed to create streaming response: {}", e))
        })
}

/// Builds realtime streaming response that immediately returns an SSE stream.
///
/// Unlike `build_enhanced_streaming_response` which collects all events first,
/// this function returns a stream that reads from the receiver in real-time.
/// Events are sent to the client as they are produced by the background task.
fn build_realtime_streaming_response(
    event_receiver: mpsc::Receiver<String>,
    request_id: &str,
) -> ServerResult<Response<Body>> {
    use tokio_stream::wrappers::ReceiverStream;

    let request_id_owned = request_id.to_string();

    // Convert mpsc::Receiver to a Stream that yields events in real-time
    let stream = ReceiverStream::new(event_receiver)
        .map(|s| Ok::<_, std::convert::Infallible>(s.into_bytes()));

    Response::builder()
        .header(CONTENT_TYPE, "text/event-stream")
        .header("Cache-Control", "no-cache")
        .header("Connection", "keep-alive")
        .header("X-Enhanced-Stream", "true")
        .header("X-Realtime-Stream", "true")
        .status(StatusCode::OK)
        .body(Body::from_stream(stream))
        .map_err(|e| {
            dual_error!(
                "Failed to create realtime streaming response: {} - request_id: {}",
                e,
                request_id_owned
            );
            ServerError::Operation(format!(
                "Failed to create realtime streaming response: {}",
                e
            ))
        })
}

// ============================================================================
// Direct Answer Response Builder
// ============================================================================

/// Builds HTTP response for direct answers (simple queries).
///
/// This function handles both streaming and non-streaming responses for queries
/// that can be answered directly without task planning and execution.
fn build_direct_answer_response(
    answer: &str,
    request_id: &str,
    stream: bool,
) -> ServerResult<Response<Body>> {
    let id = gen_chat_id();
    let created = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    if stream {
        // Streaming response
        let chunks = gen_chunks_with_formatting(answer, 10);
        let model = "direct-answer".to_string();
        let chunks_len = chunks.len();

        let request_id_owned = request_id.to_string();
        let stream = stream::iter(chunks.into_iter().enumerate().map(move |(i, chunk)| {
            let mut chat_completion_chunk = ChatCompletionChunk {
                id: id.clone(),
                object: "chat.completion.chunk".to_string(),
                created,
                model: model.clone(),
                system_fingerprint: "fp_direct_answer".to_string(),
                choices: vec![ChatCompletionChunkChoice {
                    index: i as u32,
                    delta: ChatCompletionChunkChoiceDelta {
                        role: ChatCompletionRole::Assistant,
                        content: Some(chunk),
                        tool_calls: vec![],
                    },
                    logprobs: None,
                    finish_reason: None,
                }],
                usage: None,
            };

            if i == chunks_len - 1 {
                chat_completion_chunk.choices[0].finish_reason =
                    Some(endpoints::common::FinishReason::stop);
            }

            let json_str = serde_json::to_string(&chat_completion_chunk).unwrap();
            format!("data: {json_str}\n\n")
        }))
        .chain(stream::once(async { "data: [DONE]\n\n".to_string() }))
        .map(|s| Ok::<_, std::convert::Infallible>(s.into_bytes()));

        Response::builder()
            .header(CONTENT_TYPE, "text/event-stream")
            .header("Cache-Control", "no-cache")
            .header("Connection", "keep-alive")
            .status(StatusCode::OK)
            .body(Body::from_stream(stream))
            .map_err(|e| {
                dual_error!(
                    "Failed to create direct answer streaming response: {} - request_id: {}",
                    e,
                    request_id_owned
                );
                ServerError::Operation(format!(
                    "Failed to create direct answer streaming response: {}",
                    e
                ))
            })
    } else {
        // Non-streaming response
        let chat_completion = ChatCompletionObject {
            id,
            object: "chat.completion".to_string(),
            created,
            model: "direct-answer".to_string(),
            choices: vec![endpoints::chat::ChatCompletionObjectChoice {
                index: 0,
                message: endpoints::chat::ChatCompletionObjectMessage {
                    role: ChatCompletionRole::Assistant,
                    content: Some(answer.to_string()),
                    tool_calls: vec![],
                    function_call: None,
                },
                finish_reason: endpoints::common::FinishReason::stop,
                logprobs: None,
            }],
            usage: endpoints::common::Usage {
                prompt_tokens: 0,
                completion_tokens: 0,
                total_tokens: 0,
            },
        };

        let response_body = serde_json::to_string(&chat_completion)
            .map_err(|e| ServerError::Operation(format!("Failed to serialize response: {}", e)))?;

        Response::builder()
            .header(CONTENT_TYPE, "application/json")
            .status(StatusCode::OK)
            .body(Body::from(response_body))
            .map_err(|e| {
                dual_error!(
                    "Failed to create direct answer response: {} - request_id: {}",
                    e,
                    request_id
                );
                ServerError::Operation(format!("Failed to create direct answer response: {}", e))
            })
    }
}

// ============================================================================
// Replanning Helper Functions (R5.2)
// ============================================================================

/// Extracts failed subtask information from the plan trace.
fn extract_failed_subtasks(trace: &PlanTrace) -> Vec<FailedSubtaskInfo> {
    use crate::chat::planner::SubTaskStatus;
    trace
        .subtask_traces
        .iter()
        .filter_map(|t| {
            if let SubTaskStatus::Failed(ref error) = t.status {
                Some(FailedSubtaskInfo {
                    id: t.subtask_id,
                    description: t.description.clone(),
                    error: error.clone(),
                    retry_count: t.retry_count,
                })
            } else {
                None
            }
        })
        .collect()
}

/// Extracts pending subtask information from the plan.
fn extract_pending_subtasks(plan: &TaskPlan, completed_ids: &HashSet<usize>) -> Vec<SubtaskInfo> {
    plan.subtasks
        .iter()
        .filter(|s| !completed_ids.contains(&s.id))
        .map(|s| SubtaskInfo {
            id: s.id,
            description: s.description.clone(),
            dependencies: s.dependencies.clone(),
        })
        .collect()
}

/// Executes the replanning process.
#[allow(clippy::too_many_arguments)]
async fn execute_replan(
    replanner: &DynamicReplanner,
    original_goal: &str,
    pending_subtasks: Vec<SubtaskInfo>,
    subtask_results: &[(usize, String)],
    trace: &PlanTrace,
    trigger: ReplanTrigger,
    remaining_time: Option<Duration>,
    request_id: &str,
) -> ServerResult<crate::reflection::ReplanResult> {
    use std::collections::HashMap;

    dual_info!(
        "🔄 Starting replan due to: {} - request_id: {}",
        trigger.description(),
        request_id
    );

    // Build completed results map
    let completed_results: HashMap<usize, String> = subtask_results.iter().cloned().collect();

    // Extract failed subtasks from trace
    let failed_subtasks = extract_failed_subtasks(trace);

    // Build replan context
    let context = ReplanContext {
        original_goal: original_goal.to_string(),
        completed_results,
        failed_subtasks,
        pending_subtasks,
        trigger,
        remaining_time,
    };

    // Execute replanning
    replanner.replan(&context).await
}

/// Applies a new plan from replanning result.
#[allow(dead_code)]
fn apply_new_plan(
    current_plan: &mut TaskPlan,
    replan_result: crate::reflection::ReplanResult,
    request_id: &str,
) {
    use crate::chat::planner::SubTaskStatus;

    dual_info!(
        "📝 Applying new plan: preserved={}, added={}, removed={} - request_id: {}",
        replan_result.preserved_subtasks.len(),
        replan_result.added_subtasks.len(),
        replan_result.removed_subtasks.len(),
        request_id
    );

    // Create new subtasks from replan result
    let mut new_subtasks: Vec<SubTask> = Vec::new();

    for new_subtask in replan_result.new_subtasks {
        let subtask = if new_subtask.preserved {
            // Find the original subtask and preserve its state
            if let Some(original_id) = new_subtask.original_id {
                if let Some(original) = current_plan.subtasks.iter().find(|s| s.id == original_id) {
                    let mut s = original.clone();
                    s.id = new_subtask.id;
                    s.dependencies = new_subtask.dependencies;
                    s
                } else {
                    SubTask {
                        id: new_subtask.id,
                        description: new_subtask.description,
                        dependencies: new_subtask.dependencies,
                        required_tools: vec![],
                        recommended_skill: None,
                        status: SubTaskStatus::Pending,
                        result: None,
                    }
                }
            } else {
                SubTask {
                    id: new_subtask.id,
                    description: new_subtask.description,
                    dependencies: new_subtask.dependencies,
                    required_tools: vec![],
                    recommended_skill: None,
                    status: SubTaskStatus::Pending,
                    result: None,
                }
            }
        } else {
            SubTask {
                id: new_subtask.id,
                description: new_subtask.description,
                dependencies: new_subtask.dependencies,
                required_tools: vec![],
                recommended_skill: None,
                status: SubTaskStatus::Pending,
                result: None,
            }
        };
        new_subtasks.push(subtask);
    }

    // Update the plan
    current_plan.subtasks = new_subtasks;

    // Recalculate execution order
    current_plan.execution_order = current_plan.subtasks.iter().map(|s| s.id).collect();

    dual_info!(
        "✅ New plan applied with {} subtasks - request_id: {}",
        current_plan.subtasks.len(),
        request_id
    );
}

// ============================================================================
// Sub-Agent Execution Mode
// ============================================================================

/// Execute a subtask using a Sub-Agent.
///
/// This function creates a Sub-Agent to execute the subtask, with support for:
/// - Context injection from previous subtask results
/// - Load active skills for a subtask based on its `recommended_skill` field.
///
/// Used by Sub-Agent executor to handle internal tool calls (e.g., `internal__skill_run_script`).
async fn load_skills_for_subtask(subtask: &SubTask, request_id: &str) -> Vec<LoadedSkill> {
    if let Some(skill_name) = &subtask.recommended_skill
        && let Ok(registry) = SkillRegistry::global()
    {
        if let Some(skill) = registry.get(skill_name).await {
            dual_debug!(
                "🎯 Loaded skill '{}' for Sub-Agent subtask {} - request_id: {}",
                skill_name,
                subtask.id,
                request_id
            );
            return vec![skill];
        } else {
            dual_warn!(
                "⚠️ Recommended skill '{}' not found for subtask {} - request_id: {}",
                skill_name,
                subtask.id,
                request_id
            );
        }
    }

    Vec::new()
}

/// - Tool inheritance with blacklist filtering
/// - Timeout management with graceful exit
/// - Progress tracking and event emission
#[allow(clippy::too_many_arguments)]
async fn execute_subtask_via_subagent(
    state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    headers: &HeaderMap,
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    available_tools: &[ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    timeout: Duration,
    cancel_token: &CancellationToken,
    request_id: &str,
    subtask_trace: &mut SubtaskTrace,
    model: &str,
    emitter: &dyn EventEmitter,
    subagent_config: Option<&SubAgentSystemConfig>,
    plan_pause_tracker: Option<Arc<AtomicU64>>,
    file_attachments: &[FileAttachmentInfo],
) -> ServerResult<String> {
    let config = subagent_config.ok_or_else(|| {
        ServerError::Operation(
            "Sub-Agent configuration not found for subagent execution mode".to_string(),
        )
    })?;
    let executor_config = &config.subtask_executor;
    let context_config = &config.context;

    dual_info!(
        "🤖 Executing subtask {} via Sub-Agent - request_id: {}",
        subtask.id,
        request_id
    );

    // 1. Build context from previous results
    let context = build_subtask_context(
        subtask,
        previous_results,
        context_config,
        skills_summaries,
        file_attachments,
    );

    // 2. Calculate effective timeout
    let effective_timeout = if executor_config.inherit_remaining_time {
        timeout.min(Duration::from_secs(executor_config.timeout_secs))
    } else {
        Duration::from_secs(executor_config.timeout_secs)
    };

    // 3. Filter tools based on inheritance and blacklist
    // IMPORTANT: Always filter out Sub-Agent tools to prevent recursive spawning
    let filtered_tools = if executor_config.inherit_tools {
        available_tools
            .iter()
            .filter(|t| {
                !is_tool_blocked(&t.name, &executor_config.blocked_tools)
                    && !is_subagent_tool(&t.name)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        // Only allow explicitly required tools (but never Sub-Agent tools)
        available_tools
            .iter()
            .filter(|t| subtask.required_tools.contains(&t.name) && !is_subagent_tool(&t.name))
            .cloned()
            .collect::<Vec<_>>()
    };

    // Debug: 打印工具过滤结果
    dual_debug!(
        "Sub-Agent tool filtering: inherit_tools={}, available={}, filtered={}, tool_names={:?} - request_id: {}",
        executor_config.inherit_tools,
        available_tools.len(),
        filtered_tools.len(),
        filtered_tools.iter().map(|t| &t.name).collect::<Vec<_>>(),
        request_id
    );

    // 4. Load active skills for internal tool execution and system prompt enrichment
    let active_skills = load_skills_for_subtask(subtask, request_id).await;

    // 5. Create Sub-Agent manager with configuration
    let subagent_manager = Arc::new(SubAgentManager::new(config.clone()));

    // 6. Build spawn configuration
    let spawn_config = SubAgentSpawnConfig {
        timeout_secs: Some(effective_timeout.as_secs()),
        max_iterations: Some(config.default_max_iterations),
        tool_access: SubAgentToolAccess {
            allowed_tools: None, // Inherit all
            blocked_tools: executor_config.blocked_tools.iter().cloned().collect(),
            inherit_from_parent: true,
        },
        wait_for_completion: true,
    };

    // 7. Build system prompt for the Sub-Agent (with full skill content if loaded)
    let system_prompt =
        build_subagent_system_prompt(subtask, skills_summaries, &active_skills, file_attachments);

    // 8. Spawn the Sub-Agent (with subtask_id for HITL display)
    let subagent_id = subagent_manager
        .spawn_with_subtask_id(
            format!("subtask-{}", subtask.id),
            system_prompt.clone(),
            context.clone(),
            Some(spawn_config),
            None, // No parent
            Some(subtask.id),
        )
        .await?;

    let subagent_name = format!("Subtask-{}", subtask.id);
    let start_time = std::time::Instant::now();

    // 9. Emit spawn event
    emitter
        .emit_subagent_spawned(
            subagent_id.as_ref(),
            &subagent_name,
            &subtask.description,
            None, // No parent
            0,    // Depth 0
        )
        .await;

    // 10. Create executor and run
    // Extract user_id from headers (set by frontend in X-User-ID header)
    let user_id = headers
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    let executor = SubAgentExecutor::with_hitl_context(
        state.clone(),
        chat_server.clone(),
        headers.clone(),
        subagent_manager.clone(),
        filtered_tools,
        model.to_string(),
        request_id.to_string(), // Use request_id as conversation_id
        user_id,
    )
    .with_active_skills(active_skills);

    let subagent_context = SubAgentContext::new(
        subagent_id.clone(),
        subagent_name.clone(),
        system_prompt.clone(),
        context,
    );

    let result = executor
        .execute(
            &subagent_id,
            subagent_context,
            effective_timeout,
            config.default_max_iterations,
            cancel_token,
            emitter,
            plan_pause_tracker,
        )
        .await;

    // 11. Handle result
    let duration_ms = start_time.elapsed().as_millis() as u64;

    match result {
        Ok(subagent_result) => {
            // Update trace metrics
            let metrics = &subagent_result.metrics;
            let iterations = metrics.total_iterations;

            dual_debug!(
                "Sub-Agent metrics: iterations={}, prompt_tokens={}, completion_tokens={} - request_id: {}",
                metrics.total_iterations,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                request_id
            );

            // Record Sub-Agent metrics in SubtaskTrace using IterationTrace
            // For Sub-Agent mode, we create a single summary iteration trace that
            // captures all the metrics from the Sub-Agent execution.
            for i in 0..iterations {
                let mut iter_trace = IterationTrace::new(i + 1);
                // Distribute tokens across iterations (first iteration gets all for simplicity)
                if i == 0 {
                    iter_trace.llm_tokens =
                        TokenUsage::new(metrics.prompt_tokens, metrics.completion_tokens);
                }
                iter_trace.duration = Duration::from_millis(duration_ms / iterations.max(1) as u64);
                subtask_trace.add_iteration(iter_trace);
            }
            // Ensure at least one iteration is recorded if iterations is 0 but execution succeeded
            if iterations == 0 && metrics.prompt_tokens > 0 {
                let mut iter_trace = IterationTrace::new(1);
                iter_trace.llm_tokens =
                    TokenUsage::new(metrics.prompt_tokens, metrics.completion_tokens);
                iter_trace.duration = Duration::from_millis(duration_ms);
                subtask_trace.add_iteration(iter_trace);
            }

            // Emit completion event
            emitter
                .emit_subagent_completed(
                    subagent_id.as_ref(),
                    &subagent_name,
                    &subagent_result.output,
                    iterations,
                    duration_ms,
                )
                .await;

            dual_info!(
                "✅ Subtask {} completed via Sub-Agent - request_id: {}",
                subtask.id,
                request_id
            );

            Ok(subagent_result.output)
        }
        Err(e) => {
            // Use WARN for user interruption, ERROR for other failures
            if matches!(e, ServerError::UserInterrupted(_)) {
                dual_warn!(
                    "🛑 Subtask {} interrupted by user via Sub-Agent: {} - request_id: {}",
                    subtask.id,
                    e,
                    request_id
                );
            } else {
                dual_error!(
                    "❌ Subtask {} failed via Sub-Agent: {} - request_id: {}",
                    subtask.id,
                    e,
                    request_id
                );
            }

            // Emit failure event
            emitter
                .emit_subagent_failed(
                    subagent_id.as_ref(),
                    &subagent_name,
                    &e.to_string(),
                    0, // Unknown iterations on failure
                    duration_ms,
                )
                .await;

            Err(e)
        }
    }
}

// ============================================================================
// Retry and Graceful Exit
// ============================================================================

/// Result of a subtask execution with retry information.
#[derive(Debug, Clone)]
pub struct RetryableSubtaskResult {
    /// The output of the subtask
    pub output: String,
    /// Number of retry attempts made
    pub retry_count: u32,
    /// Whether the execution was successful
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
}

/// Execute a subtask with retry support using the respawn strategy.
///
/// This function wraps `execute_subtask_via_subagent` with retry logic:
/// - On failure, respawns a new Sub-Agent with failure context injected
/// - Uses exponential backoff between retries
/// - Respects the configured failure policy
#[allow(clippy::too_many_arguments)]
pub async fn execute_subtask_with_retry(
    state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    headers: &HeaderMap,
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    available_tools: &[ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    timeout: Duration,
    cancel_token: &CancellationToken,
    request_id: &str,
    subtask_trace: &mut SubtaskTrace,
    model: &str,
    emitter: &dyn EventEmitter,
    subagent_config: Option<&SubAgentSystemConfig>,
    plan_pause_tracker: Option<Arc<AtomicU64>>,
    file_attachments: &[FileAttachmentInfo],
) -> ServerResult<RetryableSubtaskResult> {
    let config = subagent_config.ok_or_else(|| {
        ServerError::Operation("Sub-Agent configuration required for retry execution".to_string())
    })?;
    let executor_config = &config.subtask_executor;

    // Check if retry mode is "respawn"
    let use_respawn = executor_config.is_respawn_retry();
    let max_retries = executor_config.max_retries;
    let retry_delay_ms = executor_config.retry_delay_ms;
    let inject_failure_context = executor_config.inject_failure_context;

    let mut last_error: Option<String> = None;
    let mut retry_count = 0u32;

    while retry_count <= max_retries {
        // Check cancellation before each attempt
        if cancel_token.is_cancelled() {
            return Err(ServerError::Operation(
                "Request cancelled during retry".to_string(),
            ));
        }

        // Build failure context for retry attempts
        let failure_context = if retry_count > 0 && inject_failure_context {
            last_error.as_ref().map(|err| {
                format!(
                    "\n\n## Previous Attempt Failed (Attempt {}/{})\n\n**Error:** {}\n\n**Note:** Please try a different approach to avoid the same error.",
                    retry_count,
                    max_retries + 1,
                    err
                )
            })
        } else {
            None
        };

        // Log retry attempt
        if retry_count > 0 {
            dual_info!(
                "🔄 Retrying subtask {} via respawn (attempt {}/{}) - request_id: {}",
                subtask.id,
                retry_count + 1,
                max_retries + 1,
                request_id
            );

            // Emit status event for retry
            emitter
                .emit_status(
                    ExecutionPhase::Executing,
                    &format!(
                        "Retrying subtask {} (attempt {}/{})",
                        subtask.id,
                        retry_count + 1,
                        max_retries + 1
                    ),
                    Some(subtask.id),
                    None,
                    None,
                )
                .await;
        }

        // Execute with optional failure context
        let result = execute_subtask_via_subagent_with_context(
            state,
            chat_server,
            headers,
            subtask,
            previous_results,
            available_tools,
            skills_summaries,
            timeout,
            cancel_token,
            request_id,
            subtask_trace,
            model,
            emitter,
            subagent_config,
            failure_context.as_deref(),
            plan_pause_tracker.clone(),
            file_attachments,
        )
        .await;

        match result {
            Ok(output) => {
                // Success
                if retry_count > 0 {
                    dual_info!(
                        "✅ Subtask {} succeeded after {} retries - request_id: {}",
                        subtask.id,
                        retry_count,
                        request_id
                    );
                }
                return Ok(RetryableSubtaskResult {
                    output,
                    retry_count,
                    success: true,
                    error: None,
                });
            }
            Err(e) => {
                let error_msg = e.to_string();
                last_error = Some(error_msg.clone());
                retry_count += 1;

                // Check if we should retry
                if retry_count <= max_retries && use_respawn {
                    // Calculate exponential backoff delay
                    let delay = Duration::from_millis(retry_delay_ms * (1 << (retry_count - 1)));
                    dual_debug!(
                        "Waiting {:?} before retry attempt {} - request_id: {}",
                        delay,
                        retry_count + 1,
                        request_id
                    );
                    tokio::time::sleep(delay).await;
                } else {
                    // No more retries
                    break;
                }
            }
        }
    }

    // All retries exhausted, handle based on failure policy
    let error_msg = last_error.unwrap_or_else(|| "Unknown error".to_string());

    dual_warn!(
        "⚠️ Subtask {} failed after {} retries: {} - request_id: {}",
        subtask.id,
        retry_count.saturating_sub(1),
        error_msg,
        request_id
    );

    // Determine action based on failure policy
    let failure_policy = &executor_config.failure_policy;

    if failure_policy.should_skip_after_retry() {
        // Skip: return a placeholder result
        Ok(RetryableSubtaskResult {
            output: format!(
                "[Subtask {} skipped after {} retries due to error: {}]",
                subtask.id,
                retry_count.saturating_sub(1),
                error_msg
            ),
            retry_count: retry_count.saturating_sub(1),
            success: false,
            error: Some(error_msg),
        })
    } else {
        // Fail: propagate the error
        Err(ServerError::Operation(format!(
            "Subtask {} failed after {} retries: {}",
            subtask.id,
            retry_count.saturating_sub(1),
            error_msg
        )))
    }
}

/// Execute a subtask via Sub-Agent with optional failure context injection.
///
/// This is a wrapper around `execute_subtask_via_subagent` that allows injecting
/// additional context about previous failures for retry attempts.
#[allow(clippy::too_many_arguments)]
async fn execute_subtask_via_subagent_with_context(
    state: &Arc<AppState>,
    chat_server: &crate::server::TargetServerInfo,
    headers: &HeaderMap,
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    available_tools: &[ToolDescription],
    skills_summaries: Option<&[SkillSummary]>,
    timeout: Duration,
    cancel_token: &CancellationToken,
    request_id: &str,
    subtask_trace: &mut SubtaskTrace,
    model: &str,
    emitter: &dyn EventEmitter,
    subagent_config: Option<&SubAgentSystemConfig>,
    failure_context: Option<&str>,
    plan_pause_tracker: Option<Arc<AtomicU64>>,
    file_attachments: &[FileAttachmentInfo],
) -> ServerResult<String> {
    let config = subagent_config.ok_or_else(|| {
        ServerError::Operation(
            "Sub-Agent configuration not found for subagent execution mode".to_string(),
        )
    })?;
    let executor_config = &config.subtask_executor;
    let context_config = &config.context;

    dual_info!(
        "🤖 Executing subtask {} via Sub-Agent{} - request_id: {}",
        subtask.id,
        if failure_context.is_some() {
            " (with failure context)"
        } else {
            ""
        },
        request_id
    );

    // 1. Build context from previous results
    let mut context = build_subtask_context(
        subtask,
        previous_results,
        context_config,
        skills_summaries,
        file_attachments,
    );

    // 1.5. Inject failure context if provided
    if let Some(failure_ctx) = failure_context {
        context.push_str(failure_ctx);
    }

    // 2. Calculate effective timeout
    let effective_timeout = if executor_config.inherit_remaining_time {
        timeout.min(Duration::from_secs(executor_config.timeout_secs))
    } else {
        Duration::from_secs(executor_config.timeout_secs)
    };

    // 3. Filter tools based on inheritance and blacklist
    let filtered_tools = if executor_config.inherit_tools {
        available_tools
            .iter()
            .filter(|t| !is_tool_blocked(&t.name, &executor_config.blocked_tools))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        available_tools
            .iter()
            .filter(|t| subtask.required_tools.contains(&t.name))
            .cloned()
            .collect::<Vec<_>>()
    };

    // 4. Load active skills for internal tool execution and system prompt enrichment
    let active_skills = load_skills_for_subtask(subtask, request_id).await;

    // 5. Create Sub-Agent manager with configuration
    let subagent_manager = Arc::new(SubAgentManager::new(config.clone()));

    // 6. Build spawn configuration
    let spawn_config = SubAgentSpawnConfig {
        timeout_secs: Some(effective_timeout.as_secs()),
        max_iterations: Some(config.default_max_iterations),
        tool_access: SubAgentToolAccess {
            allowed_tools: None,
            blocked_tools: executor_config.blocked_tools.iter().cloned().collect(),
            inherit_from_parent: true,
        },
        wait_for_completion: true,
    };

    // 7. Build system prompt for the Sub-Agent (with full skill content if loaded)
    let system_prompt =
        build_subagent_system_prompt(subtask, skills_summaries, &active_skills, file_attachments);

    // 8. Spawn the Sub-Agent (with subtask_id for HITL display)
    let subagent_id = subagent_manager
        .spawn_with_subtask_id(
            format!("subtask-{}", subtask.id),
            system_prompt.clone(),
            context.clone(),
            Some(spawn_config),
            None,
            Some(subtask.id),
        )
        .await?;

    let subagent_name = format!("Subtask-{}", subtask.id);
    let start_time = std::time::Instant::now();

    // 9. Emit spawn event
    emitter
        .emit_subagent_spawned(
            subagent_id.as_ref(),
            &subagent_name,
            &subtask.description,
            None,
            0,
        )
        .await;

    // 10. Create executor and run
    // Extract user_id from headers (set by frontend in X-User-ID header)
    let user_id = headers
        .get("X-User-ID")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("anonymous")
        .to_string();

    let executor = SubAgentExecutor::with_hitl_context(
        state.clone(),
        chat_server.clone(),
        headers.clone(),
        subagent_manager.clone(),
        filtered_tools,
        model.to_string(),
        request_id.to_string(), // Use request_id as conversation_id
        user_id,
    )
    .with_active_skills(active_skills);

    let subagent_context = SubAgentContext::new(
        subagent_id.clone(),
        subagent_name.clone(),
        system_prompt.clone(),
        context,
    );

    let result = executor
        .execute(
            &subagent_id,
            subagent_context,
            effective_timeout,
            config.default_max_iterations,
            cancel_token,
            emitter,
            plan_pause_tracker,
        )
        .await;

    // 10. Handle result
    let duration_ms = start_time.elapsed().as_millis() as u64;

    match result {
        Ok(subagent_result) => {
            let metrics = &subagent_result.metrics;
            let iterations = metrics.total_iterations;

            dual_debug!(
                "Sub-Agent metrics: iterations={}, prompt_tokens={}, completion_tokens={} - request_id: {}",
                metrics.total_iterations,
                metrics.prompt_tokens,
                metrics.completion_tokens,
                request_id
            );

            // Record Sub-Agent metrics in SubtaskTrace using IterationTrace
            // For Sub-Agent mode, we create iteration traces that capture the metrics
            for i in 0..iterations {
                let mut iter_trace = IterationTrace::new(i + 1);
                // First iteration gets all tokens for simplicity
                if i == 0 {
                    iter_trace.llm_tokens =
                        TokenUsage::new(metrics.prompt_tokens, metrics.completion_tokens);
                }
                iter_trace.duration = Duration::from_millis(duration_ms / iterations.max(1) as u64);
                subtask_trace.add_iteration(iter_trace);
            }
            // Ensure at least one iteration is recorded if iterations is 0 but execution succeeded
            if iterations == 0 && metrics.prompt_tokens > 0 {
                let mut iter_trace = IterationTrace::new(1);
                iter_trace.llm_tokens =
                    TokenUsage::new(metrics.prompt_tokens, metrics.completion_tokens);
                iter_trace.duration = Duration::from_millis(duration_ms);
                subtask_trace.add_iteration(iter_trace);
            }

            emitter
                .emit_subagent_completed(
                    subagent_id.as_ref(),
                    &subagent_name,
                    &subagent_result.output,
                    iterations,
                    duration_ms,
                )
                .await;

            // Update trace retry count
            subtask_trace.retry_count = 0; // Will be updated by caller

            dual_info!(
                "✅ Subtask {} completed via Sub-Agent - request_id: {}",
                subtask.id,
                request_id
            );

            Ok(subagent_result.output)
        }
        Err(e) => {
            dual_error!(
                "❌ Subtask {} failed via Sub-Agent: {} - request_id: {}",
                subtask.id,
                e,
                request_id
            );

            emitter
                .emit_subagent_failed(
                    subagent_id.as_ref(),
                    &subagent_name,
                    &e.to_string(),
                    0,
                    duration_ms,
                )
                .await;

            Err(e)
        }
    }
}

/// Check if a tool is blocked by the blocked_tools list.
///
/// This function supports both full tool names (e.g., "internal__spawn_sub_agent")
/// and short names (e.g., "spawn_sub_agent") for user convenience.
fn is_tool_blocked(tool_name: &str, blocked_tools: &[String]) -> bool {
    // Check exact match first
    if blocked_tools.iter().any(|b| b == tool_name) {
        return true;
    }

    // Check if short name matches (tool_name may be "prefix__short_name")
    // Extract the short name by finding the last "__" separator
    if let Some(pos) = tool_name.rfind("__") {
        let short_name = &tool_name[pos + 2..];
        if blocked_tools.iter().any(|b| b == short_name) {
            return true;
        }
    }

    false
}

/// Build context for a subtask from previous results.
///
/// This function constructs the context string that will be passed to the Sub-Agent,
/// including results from dependent subtasks and the current task description.
fn build_subtask_context(
    subtask: &SubTask,
    previous_results: &[(usize, String)],
    context_config: &crate::subagent::SubAgentContextConfig,
    skills_summaries: Option<&[SkillSummary]>,
    file_attachments: &[FileAttachmentInfo],
) -> String {
    let mut context = String::new();

    // Inject previous results if configured
    if context_config.is_inject_mode() && !previous_results.is_empty() {
        context.push_str("## Previous Subtask Results\n\n");

        for (id, result) in previous_results {
            // Check if this subtask depends on the result
            if subtask.dependencies.contains(id) {
                let result_text = if context_config.use_summary
                    && result.len() > context_config.max_inject_tokens * 4
                {
                    // Truncate long results (TODO: implement proper summarization)
                    let truncated =
                        &result[..result.len().min(context_config.max_inject_tokens * 4)];
                    format!("{truncated}\n\n[Result truncated due to length]")
                } else {
                    result.clone()
                };
                context.push_str(&format!("### Subtask {} Result\n{}\n\n", id, result_text));
            }
        }
    }

    // Add current task description
    context.push_str("## Your Task\n\n");
    context.push_str(&subtask.description);
    context.push('\n');

    // Add recommended skill hint if available
    if let Some(skill) = &subtask.recommended_skill {
        context.push_str(&format!("\n**Recommended Skill**: {}\n", skill));

        // Add skill description if available
        if let Some(summaries) = skills_summaries
            && let Some(summary) = summaries.iter().find(|s| &s.name == skill)
        {
            context.push_str(&format!("*{}*\n", summary.description));
        }
    }

    // Add required tools hint
    if !subtask.required_tools.is_empty() {
        context.push_str(&format!(
            "\n**Required Tools**: {}\n",
            subtask.required_tools.join(", ")
        ));
    }

    // Inject file attachment content (text-only format for Sub-Agent mode)
    if !file_attachments.is_empty() {
        context.push_str("\n## User Attached Files\n\n");
        for file in file_attachments {
            context.push_str(&resolve_file_as_text(file));
            context.push('\n');
        }
    }

    context
}

/// Build system prompt for a Sub-Agent executing a subtask.
///
/// When `active_skills` is non-empty, injects the full SKILL.md content
/// (via `SkillInjector::phase2_injection`) so the Sub-Agent knows exact
/// script filenames, usage instructions, and other skill details.
fn build_subagent_system_prompt(
    subtask: &SubTask,
    skills_summaries: Option<&[SkillSummary]>,
    active_skills: &[LoadedSkill],
    file_attachments: &[FileAttachmentInfo],
) -> String {
    let mut prompt = String::new();

    prompt
        .push_str("You are a specialized Sub-Agent tasked with completing a specific subtask.\n\n");

    prompt.push_str("## Guidelines\n\n");
    prompt.push_str("1. Focus on completing the assigned task efficiently\n");
    prompt.push_str("2. Use the available tools when necessary\n");
    prompt.push_str("3. Provide a clear, concise result when done\n");
    prompt.push_str("4. If you encounter errors, try alternative approaches\n\n");

    // Add full skill content if loaded, otherwise fall back to summary
    if !active_skills.is_empty() {
        // Inject full SKILL.md content (same as Plan mode Phase 2)
        for skill in active_skills {
            prompt.push_str(&SkillInjector::phase2_injection(skill));
            prompt.push_str("\n\n");
        }
    } else if let Some(skill) = &subtask.recommended_skill
        && let Some(summaries) = skills_summaries
        && let Some(summary) = summaries.iter().find(|s| &s.name == skill)
    {
        prompt.push_str(&format!("## Recommended Skill: {}\n", skill));
        prompt.push_str(&format!("{}\n\n", summary.description));
    }

    // Inform Sub-Agent about attached files
    if !file_attachments.is_empty() {
        prompt.push_str("## User Attached Files\n\n");
        prompt.push_str("The user has attached the following files to their request. ");
        prompt.push_str("Text and code file contents are included in the task context. ");
        prompt.push_str(
            "Image files are referenced by path and can be accessed via tools if needed.\n\n",
        );
        for file in file_attachments {
            prompt.push_str(&format!(
                "- {} ({}, path: {})\n",
                file.basename, file.mime_type, file.filename
            ));
        }
        prompt.push('\n');
    }

    prompt
}

// ============================================================================
// Parallel Execution Scheduler
// ============================================================================

/// A group of subtasks that can be executed in parallel.
///
/// All subtasks in the same group have their dependencies satisfied by
/// previous groups, so they can safely run concurrently.
#[derive(Debug, Clone)]
pub struct ParallelGroup {
    /// Indices of subtasks in this group
    pub subtask_indices: Vec<usize>,
}

/// Analyze subtask dependencies and group them for parallel execution.
///
/// This function performs a topological sort of subtasks based on their
/// dependencies, grouping subtasks that can be executed in parallel.
///
/// # Algorithm
///
/// 1. Start with all subtasks that have no dependencies (or all dependencies already complete)
/// 2. Mark these subtasks as the first parallel group
/// 3. Find all subtasks whose dependencies are in previous groups
/// 4. Repeat until all subtasks are grouped
///
/// # Returns
///
/// A vector of `ParallelGroup`s, where each group contains subtask indices
/// that can be executed concurrently.
pub fn analyze_dependencies(
    subtasks: &[SubTask],
    completed_subtasks: &HashSet<usize>,
) -> Vec<ParallelGroup> {
    let mut groups = Vec::new();
    let mut processed: HashSet<usize> = completed_subtasks.clone();
    let remaining: Vec<_> = subtasks
        .iter()
        .filter(|s| !completed_subtasks.contains(&s.id) && s.status == SubTaskStatus::Pending)
        .collect();

    if remaining.is_empty() {
        return groups;
    }

    // Keep grouping until all subtasks are processed
    let mut remaining_set: HashSet<usize> = remaining.iter().map(|s| s.id).collect();

    while !remaining_set.is_empty() {
        let mut group_indices = Vec::new();

        // Find all subtasks whose dependencies are satisfied
        for subtask in &remaining {
            if remaining_set.contains(&subtask.id) {
                let deps_satisfied = subtask
                    .dependencies
                    .iter()
                    .all(|dep| processed.contains(dep));

                if deps_satisfied {
                    group_indices.push(subtask.id);
                }
            }
        }

        // If no subtasks can be added, we have a cycle or all are done
        if group_indices.is_empty() {
            // Add remaining tasks as a single group (they may have circular deps)
            let remaining_indices: Vec<usize> = remaining_set.iter().copied().collect();
            if !remaining_indices.is_empty() {
                groups.push(ParallelGroup {
                    subtask_indices: remaining_indices,
                });
            }
            break;
        }

        // Mark these as processed
        for idx in &group_indices {
            processed.insert(*idx);
            remaining_set.remove(idx);
        }

        groups.push(ParallelGroup {
            subtask_indices: group_indices,
        });
    }

    groups
}

/// Context required for parallel execution of subtasks.
#[derive(Clone)]
pub struct ParallelExecutionContext {
    pub state: Arc<AppState>,
    pub chat_server: crate::server::TargetServerInfo,
    pub headers: HeaderMap,
    pub available_tools: Vec<ToolDescription>,
    pub skills_summaries: Vec<SkillSummary>,
    pub cancel_token: CancellationToken,
    pub request_id: String,
    pub model_name: String,
    pub subagent_config: Option<SubAgentSystemConfig>,
    pub rate_limiter: Option<Arc<RateLimiter>>,
    /// Event emitter for streaming events back to the client.
    pub emitter: Arc<dyn EventEmitter>,
    /// File attachments from user message.
    pub file_attachments: Vec<FileAttachmentInfo>,
}

/// Result of a single subtask execution.
#[derive(Debug, Clone)]
pub struct SubtaskExecutionResult {
    pub subtask_id: usize,
    pub result: Result<String, String>,
    pub trace: SubtaskTrace,
}

/// Execute subtasks in parallel groups using Sub-Agent mode.
///
/// This function:
/// 1. Analyzes dependencies to create parallel groups
/// 2. Executes each group concurrently (respecting max_parallel limit)
/// 3. Collects results and updates state
/// 4. Proceeds to the next group only after the current group completes
#[allow(clippy::too_many_arguments)]
pub async fn execute_subtasks_parallel(
    ctx: &ParallelExecutionContext,
    subtasks: &mut [SubTask],
    subtask_results: &mut Vec<(usize, String)>,
    completed_subtasks: &mut HashSet<usize>,
    time_budget: &TimeBudget,
    emitter: &dyn EventEmitter,
    trace: &mut PlanTrace,
    total_subtasks: usize,
    plan_pause_tracker: Arc<AtomicU64>,
) -> ServerResult<()> {
    let config = ctx.subagent_config.as_ref().ok_or_else(|| {
        ServerError::Operation(
            "Sub-Agent configuration required for parallel execution".to_string(),
        )
    })?;
    let executor_config = &config.subtask_executor;
    let max_parallel = executor_config.max_parallel;

    dual_info!(
        "🚀 Starting parallel execution with max_parallel={} - request_id: {}",
        max_parallel,
        ctx.request_id
    );

    // Analyze dependencies and create parallel groups
    let groups = analyze_dependencies(subtasks, completed_subtasks);

    dual_debug!(
        "📊 Dependency analysis: {} parallel groups - request_id: {}",
        groups.len(),
        ctx.request_id
    );

    // Execute each group
    for (group_idx, group) in groups.iter().enumerate() {
        // Check time budget
        if time_budget.is_exhausted() {
            dual_warn!(
                "Time budget exhausted during parallel execution - request_id: {}",
                ctx.request_id
            );
            return Err(ServerError::TimeBudgetExhausted {
                elapsed_secs: time_budget.elapsed().as_secs(),
            });
        }

        // Check cancellation
        if ctx.cancel_token.is_cancelled() {
            return Err(ServerError::Operation(
                "Request was cancelled by client".to_string(),
            ));
        }

        dual_info!(
            "▶️ Executing parallel group {}/{} with {} subtasks - request_id: {}",
            group_idx + 1,
            groups.len(),
            group.subtask_indices.len(),
            ctx.request_id
        );

        // Execute subtasks in this group concurrently
        let group_results = execute_parallel_group(
            ctx,
            subtasks,
            &group.subtask_indices,
            subtask_results,
            max_parallel,
            time_budget,
            emitter,
            plan_pause_tracker.clone(),
        )
        .await;

        // Process results
        for result in group_results {
            let subtask_id = result.subtask_id;

            // Add trace
            trace.add_subtask_trace(result.trace);

            match result.result {
                Ok(output) => {
                    // Update subtask status
                    if let Some(subtask) = subtasks.iter_mut().find(|s| s.id == subtask_id) {
                        subtask.complete(output.clone());
                    }
                    subtask_results.push((subtask_id, output.clone()));
                    completed_subtasks.insert(subtask_id);

                    // Emit completion event
                    emitter
                        .emit_status(
                            ExecutionPhase::Executing,
                            &format!("Completed subtask {}", subtask_id),
                            Some(subtask_id),
                            Some(completed_subtasks.len()),
                            Some(total_subtasks),
                        )
                        .await;
                }
                Err(error) => {
                    // Check if this is a user interruption (not a real error)
                    let is_user_interrupted = error.contains("User interrupted")
                        || error.contains("rejected by user")
                        || error.contains("Tool call rejected")
                        || error.contains("cancelled due to another subtask");

                    if is_user_interrupted {
                        dual_warn!(
                            "⚠️ Subtask {} interrupted by user: {} - request_id: {}",
                            subtask_id,
                            error,
                            ctx.request_id
                        );
                    } else {
                        dual_error!(
                            "❌ Subtask {} failed: {} - request_id: {}",
                            subtask_id,
                            error,
                            ctx.request_id
                        );
                    }

                    // Update subtask status
                    if let Some(subtask) = subtasks.iter_mut().find(|s| s.id == subtask_id) {
                        subtask.fail(error.clone());
                    }

                    // Handle based on failure policy
                    match executor_config.failure_policy {
                        crate::subagent::FailurePolicy::FailFast
                        | crate::subagent::FailurePolicy::Fail => {
                            // Preserve UserInterrupted error type for proper handling upstream
                            if is_user_interrupted {
                                return Err(ServerError::UserInterrupted(format!(
                                    "Subtask {} interrupted: {}",
                                    subtask_id, error
                                )));
                            }
                            return Err(ServerError::Operation(format!(
                                "Subtask {} failed: {}",
                                subtask_id, error
                            )));
                        }
                        crate::subagent::FailurePolicy::Skip
                        | crate::subagent::FailurePolicy::Ignore => {
                            // Skip this subtask, continue with others
                            dual_warn!(
                                "Skipping failed subtask {} due to failure policy - request_id: {}",
                                subtask_id,
                                ctx.request_id
                            );
                            completed_subtasks.insert(subtask_id); // Mark as "done" to unblock dependents
                        }
                        _ => {
                            // For retry policies, we'd need more complex handling
                            // For now, treat as skip
                            completed_subtasks.insert(subtask_id);
                        }
                    }
                }
            }
        }

        dual_info!(
            "✅ Parallel group {}/{} completed - request_id: {}",
            group_idx + 1,
            groups.len(),
            ctx.request_id
        );
    }

    Ok(())
}

/// Execute a single parallel group of subtasks.
///
/// Uses a semaphore to limit concurrent executions to `max_parallel`.
/// Implements fail-fast: if any subtask is rejected by user (UserInterrupted),
/// all other subtasks in the group are cancelled immediately.
#[allow(clippy::too_many_arguments)]
async fn execute_parallel_group(
    ctx: &ParallelExecutionContext,
    subtasks: &[SubTask],
    subtask_indices: &[usize],
    previous_results: &[(usize, String)],
    max_parallel: usize,
    time_budget: &TimeBudget,
    _emitter: &dyn EventEmitter,
    plan_pause_tracker: Arc<AtomicU64>,
) -> Vec<SubtaskExecutionResult> {
    use tokio::sync::Semaphore;

    let semaphore = Arc::new(Semaphore::new(max_parallel));
    let mut handles = Vec::new();

    // Create a group-level cancellation token (child of the original)
    // This allows us to cancel all subtasks in this group when one is rejected
    let group_cancel_token = ctx.cancel_token.child_token();

    // Calculate time budget for this group
    let pending_count = subtask_indices.len();
    let group_timeout = time_budget.allocate(pending_count);

    for &subtask_idx in subtask_indices {
        let subtask = match subtasks.iter().find(|s| s.id == subtask_idx) {
            Some(s) => s.clone(),
            None => continue,
        };

        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Clone context for the spawned task
        let mut ctx = ctx.clone();
        // Use the group-level cancel token instead of the original
        ctx.cancel_token = group_cancel_token.clone();
        let previous_results = previous_results.to_vec();
        let timeout = group_timeout;
        // Use the emitter from context for streaming events
        let emitter_arc: Arc<dyn EventEmitter> = ctx.emitter.clone();
        // Clone pause tracker for the spawned task
        let task_pause_tracker = plan_pause_tracker.clone();

        let handle = tokio::spawn(async move {
            let _permit = permit; // Hold until done

            // Acquire rate limit if configured
            if let Some(ref limiter) = ctx.rate_limiter
                && !limiter.acquire_request(Duration::from_secs(30)).await
            {
                return SubtaskExecutionResult {
                    subtask_id: subtask.id,
                    result: Err("Rate limit exceeded".to_string()),
                    trace: SubtaskTrace::new(subtask.id, subtask.description.clone()),
                };
            }

            let mut subtask_trace = SubtaskTrace::new(subtask.id, subtask.description.clone());
            subtask_trace.start();

            let result = execute_subtask_via_subagent(
                &ctx.state,
                &ctx.chat_server,
                &ctx.headers,
                &subtask,
                &previous_results,
                &ctx.available_tools,
                Some(&ctx.skills_summaries),
                timeout,
                &ctx.cancel_token,
                &ctx.request_id,
                &mut subtask_trace,
                &ctx.model_name,
                emitter_arc.as_ref(),
                ctx.subagent_config.as_ref(),
                Some(task_pause_tracker),
                &ctx.file_attachments,
            )
            .await;

            // Update trace based on result
            match &result {
                Ok(output) => subtask_trace.complete(output.clone()),
                Err(e) => subtask_trace.fail(e.to_string()),
            }

            SubtaskExecutionResult {
                subtask_id: subtask.id,
                result: result.map_err(|e| e.to_string()),
                trace: subtask_trace,
            }
        });

        handles.push(handle);
    }

    // Use select_all to process results as they complete
    // This allows us to cancel other tasks immediately when one is rejected
    let mut results = Vec::new();
    let mut pending_handles = handles;

    while !pending_handles.is_empty() {
        let (result, _index, remaining) = futures_util::future::select_all(pending_handles).await;

        match result {
            Ok(subtask_result) => {
                // Check if this is a user interruption (reject)
                // Error message contains "User interrupted" or "rejected by user"
                let is_user_rejected = subtask_result.result.as_ref().is_err_and(|e| {
                    e.contains("User interrupted")
                        || e.contains("rejected by user")
                        || e.contains("Tool call rejected")
                });

                if is_user_rejected {
                    dual_warn!(
                        "⚠️ Subtask {} was rejected by user, cancelling other subtasks in group",
                        subtask_result.subtask_id
                    );
                    // Cancel all other subtasks in this group
                    group_cancel_token.cancel();
                }

                results.push(subtask_result);
            }
            Err(e) => {
                dual_error!("Task join error: {:?}", e);
            }
        }

        pending_handles = remaining;
    }

    results
}

/// Check if parallel execution should be used based on configuration.
pub fn should_use_parallel_execution(
    subagent_config: Option<&SubAgentSystemConfig>,
    subtask_count: usize,
) -> bool {
    if let Some(config) = subagent_config {
        if !config.is_subagent_mode() {
            return false;
        }

        let executor_config = &config.subtask_executor;

        match executor_config.parallel_mode.as_str() {
            "auto" => {
                // Auto mode: use parallel if there are multiple subtasks
                // and max_parallel > 1
                subtask_count > 1 && executor_config.max_parallel > 1
            }
            "sequential" => false,
            "manual" => {
                // Manual mode: use parallel if max_parallel > 1
                executor_config.max_parallel > 1
            }
            _ => false,
        }
    } else {
        false
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::planner::{SubTask, SubTaskStatus};

    /// Test helper: synchronous wrapper for build_context_for_react
    fn build_context_for_react_sync(
        subtask: &SubTask,
        previous_results: &[(usize, String)],
        available_tools: &[ToolDescription],
        skills_summaries: Option<&[SkillSummary]>,
        active_skills: &[LoadedSkill],
    ) -> Vec<ChatCompletionRequestMessage> {
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(build_context_for_react(
                subtask,
                previous_results,
                available_tools,
                skills_summaries,
                active_skills,
                0,   // No reference size limit in tests
                &[], // No file attachments in tests
            ))
    }

    #[test]
    fn test_build_context_for_react_no_dependencies() {
        let subtask = SubTask::new(0, "Query weather".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let available_tools = vec![ToolDescription {
            name: "weather---weather-server".to_string(),
            description: "Get weather information".to_string(),
            ..Default::default()
        }];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &available_tools, None, &[]);

        // Should have system message + user message with task
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn test_build_context_for_react_with_dependencies() {
        let subtask =
            SubTask::new(2, "Summarize results".to_string()).with_dependencies(vec![0, 1]);
        let previous_results = vec![
            (0, "Beijing: Sunny".to_string()),
            (1, "Shanghai: Rainy".to_string()),
        ];
        let available_tools = vec![];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &available_tools, None, &[]);

        // Should have system message + context message + task message
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_build_context_for_react_with_skills_summaries() {
        let subtask = SubTask::new(0, "Query weather".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let available_tools = vec![ToolDescription {
            name: "weather---weather-server".to_string(),
            description: "Get weather information".to_string(),
            ..Default::default()
        }];
        let skills = vec![SkillSummary {
            name: "weather-query".to_string(),
            description: "Query weather for a city".to_string(),
            allowed_tools: vec![],
            parameters: None,
        }];

        let messages = build_context_for_react_sync(
            &subtask,
            &previous_results,
            &available_tools,
            Some(&skills),
            &[],
        );

        // Should have system message + user message with task
        assert_eq!(messages.len(), 2);

        // Verify skills are mentioned in system prompt
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            assert!(content.contains("Available Skills"));
            assert!(content.contains("weather-query"));
            assert!(content.contains("<use_skill>"));
        } else {
            panic!("Expected system message");
        }
    }

    #[test]
    fn test_build_context_for_react_with_active_skill() {
        use std::path::PathBuf;

        use chrono::Utc;

        let subtask = SubTask::new(0, "Query weather".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let available_tools = vec![ToolDescription {
            name: "weather---weather-server".to_string(),
            description: "Get weather information".to_string(),
            ..Default::default()
        }];
        let active_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "weather-query".to_string(),
                description: "Query weather for a city".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None,
                model: None,
                parameters: None,
            },
            content: "Use the weather tool to query weather.".to_string(),
            raw_content: "".to_string(),
            skill_dir: PathBuf::new(),
            file_path: "".to_string(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        let messages = build_context_for_react_sync(
            &subtask,
            &previous_results,
            &available_tools,
            None,
            &[active_skill],
        );

        // Should have system message + user message with task
        assert_eq!(messages.len(), 2);

        // Verify skill content is injected in system prompt
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Multi-skill format uses "Active Skills:" header
            assert!(content.contains("Active Skills: weather-query"));
            assert!(content.contains("Use the weather tool to query weather."));
        } else {
            panic!("Expected system message");
        }
    }

    #[test]
    fn test_build_tools_json() {
        let tools = vec![
            ToolDescription {
                name: "weather---weather-server".to_string(),
                description: "Get weather information".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "search---search-server".to_string(),
                description: "Search the web".to_string(),
                ..Default::default()
            },
        ];

        let tools_json = build_tools_json(&tools, None);

        assert!(tools_json.is_array());
        assert_eq!(tools_json.as_array().unwrap().len(), 2);
        assert_eq!(
            tools_json[0]["function"]["name"],
            "weather---weather-server"
        );
        assert_eq!(tools_json[1]["function"]["name"], "search---search-server");
    }

    #[test]
    fn test_build_tools_json_with_filter() {
        let tools = vec![
            ToolDescription {
                name: "mcp__weather-server__weather".to_string(),
                description: "Get weather information".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__search-server__search".to_string(),
                description: "Search the web".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp-server__Bash(git:status)".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
        ];

        // Filter to only weather tool
        let patterns = vec!["weather".to_string()];
        let tools_json = build_tools_json(&tools, Some(&patterns));

        assert!(tools_json.is_array());
        assert_eq!(tools_json.as_array().unwrap().len(), 1);
        assert_eq!(
            tools_json[0]["function"]["name"],
            "mcp__weather-server__weather"
        );
    }

    #[test]
    fn test_filter_tools_by_patterns_wildcard() {
        let tools = vec![
            ToolDescription {
                name: "mcp__mcp-server__Bash(git:status)".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp-server__Bash(git:commit)".to_string(),
                description: "Git commit".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp-server__Bash(npm:install)".to_string(),
                description: "NPM install".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__weather-server__weather".to_string(),
                description: "Get weather".to_string(),
                ..Default::default()
            },
        ];

        // Filter with wildcard pattern "Bash(git:*)"
        let patterns = vec!["Bash(git:*)".to_string()];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "mcp__mcp-server__Bash(git:status)")
        );
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "mcp__mcp-server__Bash(git:commit)")
        );
    }

    #[test]
    fn test_filter_tools_by_patterns_prefix_wildcard() {
        let tools = vec![
            ToolDescription {
                name: "Bash(git:status)---mcp-server".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "BashScript---mcp-server".to_string(),
                description: "Bash script".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "weather---weather-server".to_string(),
                description: "Get weather".to_string(),
                ..Default::default()
            },
        ];

        // Filter with prefix wildcard "Bash*"
        let patterns = vec!["Bash*".to_string()];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 2);
        assert!(
            filtered
                .iter()
                .any(|t| t.name == "Bash(git:status)---mcp-server")
        );
        assert!(filtered.iter().any(|t| t.name == "BashScript---mcp-server"));
    }

    #[test]
    fn test_filter_tools_empty_patterns() {
        let tools = vec![ToolDescription {
            name: "weather---weather-server".to_string(),
            description: "Get weather".to_string(),
            ..Default::default()
        }];

        // Empty patterns should return all tools
        let patterns: Vec<String> = vec![];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn test_filter_tools_by_patterns_full_mcp_name() {
        let tools = vec![
            ToolDescription {
                name: "mcp__cardea-weather__get_current_weather".to_string(),
                description: "Get current weather".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__cardea-calculator__sum".to_string(),
                description: "Calculate sum".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "internal__skill_run_script".to_string(),
                description: "Run script".to_string(),
                ..Default::default()
            },
        ];

        // Pattern using full MCP name (as declared in skill's allowed-tools)
        let patterns = vec!["mcp__cardea-weather__get_current_weather".to_string()];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "mcp__cardea-weather__get_current_weather");

        // Short name pattern should still work
        let patterns = vec!["get_current_weather".to_string()];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "mcp__cardea-weather__get_current_weather");

        // Mixed: full MCP name + short name
        let patterns = vec![
            "mcp__cardea-weather__get_current_weather".to_string(),
            "sum".to_string(),
        ];
        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 2);
        let names: Vec<&str> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"mcp__cardea-weather__get_current_weather"));
        assert!(names.contains(&"mcp__cardea-calculator__sum"));
    }

    #[test]
    fn test_subtask_trace_lifecycle() {
        let mut trace = SubtaskTrace::new(0, "Test task".to_string());

        assert_eq!(trace.subtask_id, 0);
        assert_eq!(trace.description, "Test task");
        assert!(matches!(trace.status, SubTaskStatus::Pending));
        assert!(trace.react_iterations.is_empty());

        // Start the trace
        trace.start();
        assert!(trace.start_time.is_some());

        // Add an iteration
        let iter_trace = IterationTrace::new(1);
        trace.add_iteration(iter_trace);
        assert_eq!(trace.react_iterations.len(), 1);

        // Complete the trace
        trace.complete("Result".to_string());
        assert!(matches!(trace.status, SubTaskStatus::Completed));
        assert_eq!(trace.result, Some("Result".to_string()));
        assert!(trace.end_time.is_some());
        assert!(trace.duration.is_some());
    }

    #[test]
    fn test_subtask_trace_failure() {
        let mut trace = SubtaskTrace::new(1, "Test task".to_string());
        trace.start();

        trace.fail("Error occurred".to_string());

        assert!(matches!(trace.status, SubTaskStatus::Failed(_)));
        if let SubTaskStatus::Failed(msg) = &trace.status {
            assert_eq!(msg, "Error occurred");
        }
    }

    #[test]
    fn test_plan_trace_lifecycle() {
        let mut trace = PlanTrace::new(
            "plan-123".to_string(),
            "Test goal".to_string(),
            vec![0, 1, 2],
        );

        assert_eq!(trace.plan_id, "plan-123");
        assert_eq!(trace.original_goal, "Test goal");
        assert_eq!(trace.execution_order, vec![0, 1, 2]);
        assert_eq!(trace.subtask_count, 3);

        // Start the trace
        trace.start();
        assert!(trace.start_time.is_some());

        // Add subtask traces
        let subtask_trace = SubtaskTrace::new(0, "Task 0".to_string());
        trace.add_subtask_trace(subtask_trace);
        assert_eq!(trace.subtask_traces.len(), 1);

        // Finalize
        trace.finalize(TraceStatus::Success);
        assert!(matches!(trace.plan_status, TraceStatus::Success));
        assert!(trace.end_time.is_some());
    }

    // ========================================================================
    // is_retryable_error tests
    // ========================================================================

    #[test]
    fn test_is_retryable_error_tool_call_retry_exhausted() {
        let error = ServerError::ToolCallRetryExhausted {
            tool_name: "test_tool".to_string(),
            attempts: 3,
            message: "Failed".to_string(),
        };
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_error_max_iterations_exceeded() {
        let error = ServerError::MaxIterationsExceeded(10);
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_error_subtask_timeout() {
        let error = ServerError::SubtaskTimeout {
            subtask_id: 1,
            timeout_secs: 60,
        };
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_error_mcp_operation() {
        let error = ServerError::McpOperation("Connection failed".to_string());
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_retryable_error_mcp_empty_content() {
        let error = ServerError::McpEmptyContent;
        assert!(is_retryable_error(&error));
    }

    #[test]
    fn test_is_not_retryable_error_operation() {
        let error = ServerError::Operation("Client cancelled".to_string());
        assert!(!is_retryable_error(&error));
    }

    #[test]
    fn test_is_not_retryable_error_plan_parse_error() {
        let error = ServerError::PlanParseError("Invalid XML".to_string());
        assert!(!is_retryable_error(&error));
    }

    #[test]
    fn test_is_not_retryable_error_cyclic_dependency() {
        let error = ServerError::CyclicDependency;
        assert!(!is_retryable_error(&error));
    }

    #[test]
    fn test_is_not_retryable_error_empty_plan() {
        let error = ServerError::EmptyPlan;
        assert!(!is_retryable_error(&error));
    }

    // ========================================================================
    // build_context_for_react additional tests
    // ========================================================================

    #[test]
    fn test_build_context_for_react_partial_dependencies() {
        // Subtask depends on 0 and 1, but only 0 is available
        let subtask =
            SubTask::new(2, "Summarize results".to_string()).with_dependencies(vec![0, 1]);
        let previous_results = vec![(0, "Beijing: Sunny".to_string())];
        let available_tools = vec![];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &available_tools, None, &[]);

        // Should have system message + context message (with partial deps) + task message
        assert_eq!(messages.len(), 3);
    }

    #[test]
    fn test_build_context_for_react_system_message_contains_task() {
        let subtask = SubTask::new(0, "Query weather for Beijing".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let available_tools = vec![ToolDescription {
            name: "weather---weather-server".to_string(),
            description: "Get weather information".to_string(),
            ..Default::default()
        }];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &available_tools, None, &[]);

        // Check system message contains task description
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            assert!(sys_msg.content().contains("Query weather for Beijing"));
            assert!(sys_msg.content().contains("weather---weather-server"));
        } else {
            panic!("First message should be system message");
        }
    }

    #[test]
    fn test_build_context_for_react_empty_tools() {
        let subtask = SubTask::new(0, "Simple task".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let available_tools: Vec<ToolDescription> = vec![];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &available_tools, None, &[]);

        assert_eq!(messages.len(), 2);
        // System message should still exist even without tools
        assert!(matches!(
            &messages[0],
            ChatCompletionRequestMessage::System(_)
        ));
    }

    // ========================================================================
    // build_tools_json additional tests
    // ========================================================================

    #[test]
    fn test_build_tools_json_empty() {
        let tools: Vec<ToolDescription> = vec![];
        let tools_json = build_tools_json(&tools, None);

        assert!(tools_json.is_array());
        assert_eq!(tools_json.as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_build_tools_json_structure() {
        // Test with default (no parameters) - uses fallback empty schema
        let tools = vec![ToolDescription {
            name: "test-tool---test-server".to_string(),
            description: "A test tool".to_string(),
            ..Default::default()
        }];

        let tools_json = build_tools_json(&tools, None);

        let tool = &tools_json[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "test-tool---test-server");
        assert_eq!(tool["function"]["description"], "A test tool");
        // Default fallback schema has empty properties
        assert_eq!(tool["function"]["parameters"]["type"], "object");
        assert!(tool["function"]["parameters"]["properties"].is_object());

        // Test with actual MCP parameters schema
        let tools_with_params = vec![ToolDescription {
            name: "search---mcp-server".to_string(),
            description: "Search tool".to_string(),
            parameters: Some(serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query"
                    }
                },
                "required": ["query"]
            })),
        }];

        let tools_json = build_tools_json(&tools_with_params, None);
        let tool = &tools_json[0];
        assert!(tool["function"]["parameters"]["properties"]["query"].is_object());
        assert_eq!(tool["function"]["parameters"]["required"][0], "query");
    }

    #[test]
    fn test_match_wildcard_pattern() {
        // Universal wildcard
        assert!(match_wildcard_pattern("*", "anything"));

        // Prefix wildcard
        assert!(match_wildcard_pattern("Bash*", "Bash"));
        assert!(match_wildcard_pattern("Bash*", "Bash(git:status)"));
        assert!(match_wildcard_pattern("Bash*", "BashScript"));
        assert!(!match_wildcard_pattern("Bash*", "NotBash"));

        // Pattern with wildcard in parentheses
        assert!(match_wildcard_pattern("Bash(git:*)", "Bash(git:status)"));
        assert!(match_wildcard_pattern("Bash(git:*)", "Bash(git:commit)"));
        assert!(!match_wildcard_pattern("Bash(git:*)", "Bash(npm:install)"));
        assert!(!match_wildcard_pattern("Bash(git:*)", "Bash"));
    }

    // ==========================================================================
    // Integration Tests: Planning Phase + Skills
    // ==========================================================================

    /// Test that skills summaries are properly injected into the planning context
    #[test]
    fn test_integration_planning_with_skills_summaries() {
        let subtask = SubTask::new(0, "Search for information".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let tools = vec![
            ToolDescription {
                name: "search---search-server".to_string(),
                description: "Search the web".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "Bash(git:status)---mcp-server".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
        ];
        let skills = vec![
            SkillSummary {
                name: "web-search".to_string(),
                description: "Perform web searches with advanced filtering".to_string(),
                allowed_tools: vec![],
                parameters: None,
            },
            SkillSummary {
                name: "git-workflow".to_string(),
                description: "Help with git operations and workflows".to_string(),
                allowed_tools: vec![],
                parameters: None,
            },
        ];

        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &tools, Some(&skills), &[]);

        // Verify system message contains skills information
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Should contain skills section
            assert!(content.contains("Available Skills"));
            // Should list both skills
            assert!(content.contains("web-search"));
            assert!(content.contains("git-workflow"));
            // Should contain usage instruction
            assert!(content.contains("<use_skill>"));
            // Should contain skill descriptions
            assert!(content.contains("Perform web searches"));
            assert!(content.contains("git operations"));
        } else {
            panic!("Expected system message as first message");
        }
    }

    /// Test that empty skills list doesn't add skills table section
    #[test]
    fn test_integration_planning_without_skills() {
        let subtask = SubTask::new(0, "Simple task".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let tools = vec![ToolDescription {
            name: "tool---server".to_string(),
            description: "A tool".to_string(),
            ..Default::default()
        }];
        let empty_skills: Vec<SkillSummary> = vec![];

        let messages = build_context_for_react_sync(
            &subtask,
            &previous_results,
            &tools,
            Some(&empty_skills),
            &[],
        );

        // Verify system message does NOT contain skills table section
        // Note: The base template still contains <use_skill> instruction,
        // but no "Available Skills" table should be present
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Should NOT have the skills table
            assert!(!content.contains("Available Skills"));
            // Should NOT list any skill names
            assert!(!content.contains("| Name |"));
        } else {
            panic!("Expected system message");
        }
    }

    // ==========================================================================
    // Integration Tests: Two-Phase Skill Loading
    // ==========================================================================

    /// Test phase 1: Skills summaries injection without active skill
    #[test]
    fn test_integration_two_phase_loading_phase1() {
        let subtask = SubTask::new(0, "Task requiring skill".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let tools = vec![
            ToolDescription {
                name: "Bash(git:status)---mcp".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "Bash(git:commit)---mcp".to_string(),
                description: "Git commit".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "Read---mcp".to_string(),
                description: "Read file".to_string(),
                ..Default::default()
            },
        ];
        let skills = vec![SkillSummary {
            name: "git-workflow".to_string(),
            description: "Git workflow assistance".to_string(),
            allowed_tools: vec![],
            parameters: None,
        }];

        // Phase 1: no active skill
        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &tools, Some(&skills), &[]);

        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Should show available skills
            assert!(content.contains("Available Skills"));
            assert!(content.contains("git-workflow"));
            // Should NOT show active skill section
            assert!(!content.contains("Active Skill:"));
        } else {
            panic!("Expected system message");
        }
    }

    /// Test phase 2: Active skill injection with full content
    #[test]
    fn test_integration_two_phase_loading_phase2() {
        use std::path::PathBuf;

        use chrono::Utc;

        let subtask = SubTask::new(0, "Git commit task".to_string());
        let previous_results: Vec<(usize, String)> = vec![];
        let tools = vec![
            ToolDescription {
                name: "Bash(git:status)---mcp".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "Bash(git:commit)---mcp".to_string(),
                description: "Git commit".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "Read---mcp".to_string(),
                description: "Read file".to_string(),
                ..Default::default()
            },
        ];

        let active_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "git-workflow".to_string(),
                description: "Git workflow assistance".to_string(),
                license: Some("MIT".to_string()),
                compatibility: Some("Requires git".to_string()),
                metadata: None,
                allowed_tools: Some("Bash(git:*) Read".to_string()),
                model: None,
                parameters: None,
            },
            content: r#"# Git Workflow

## Commit Guidelines
1. Use descriptive commit messages
2. Keep commits atomic

## Example
```bash
git add .
git commit -m "feat: add new feature"
```
"#
            .to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Phase 2: with active skill
        let messages = build_context_for_react_sync(
            &subtask,
            &previous_results,
            &tools,
            None, // No summaries needed in phase 2
            &[active_skill],
        );

        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Should show active skills section (multi-skill format)
            assert!(content.contains("Active Skills: git-workflow"));
            // Should contain skill content
            assert!(content.contains("Git Workflow"));
            assert!(content.contains("Commit Guidelines"));
            assert!(content.contains("Keep commits atomic"));
            // Should NOT show "Available Skills" section
            assert!(!content.contains("Available Skills"));
        } else {
            panic!("Expected system message");
        }
    }

    // ==========================================================================
    // Integration Tests: Tool Filtering with Skills
    // ==========================================================================

    /// Test tool filtering with skill's allowed-tools
    #[test]
    fn test_integration_tool_filtering_with_skill() {
        use std::path::PathBuf;

        use chrono::Utc;

        let tools = vec![
            ToolDescription {
                name: "mcp__mcp__Bash(git:status)".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Bash(git:commit)".to_string(),
                description: "Git commit".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Bash(npm:install)".to_string(),
                description: "NPM install".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Read".to_string(),
                description: "Read file".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Write".to_string(),
                description: "Write file".to_string(),
                ..Default::default()
            },
        ];

        let skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "git-skill".to_string(),
                description: "Git operations".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("Bash(git:*) Read".to_string()),
                model: None,
                parameters: None,
            },
            content: "Git skill content".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        let allowed_patterns = skill.metadata.get_allowed_tools();
        let tools_json = build_tools_json(&tools, Some(&allowed_patterns));

        let tool_array = tools_json.as_array().unwrap();
        // Should only include Bash(git:*) and Read
        assert_eq!(tool_array.len(), 3);

        let tool_names: Vec<&str> = tool_array
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();

        assert!(tool_names.contains(&"mcp__mcp__Bash(git:status)"));
        assert!(tool_names.contains(&"mcp__mcp__Bash(git:commit)"));
        assert!(tool_names.contains(&"mcp__mcp__Read"));
        // Should NOT include npm or Write
        assert!(!tool_names.contains(&"mcp__mcp__Bash(npm:install)"));
        assert!(!tool_names.contains(&"mcp__mcp__Write"));
    }

    /// Test tool filtering with multiple patterns
    #[test]
    fn test_integration_tool_filtering_multiple_patterns() {
        let tools = vec![
            ToolDescription {
                name: "mcp__mcp__Bash(git:status)".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Bash(npm:install)".to_string(),
                description: "NPM install".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Read".to_string(),
                description: "Read file".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Write".to_string(),
                description: "Write file".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Search".to_string(),
                description: "Search".to_string(),
                ..Default::default()
            },
        ];

        // Multiple patterns: git commands + Read + Write
        let patterns = vec![
            "Bash(git:*)".to_string(),
            "Read".to_string(),
            "Write".to_string(),
        ];

        let filtered = filter_tools_by_patterns(&tools, Some(&patterns));

        assert_eq!(filtered.len(), 3);
        let names: Vec<&str> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"mcp__mcp__Bash(git:status)"));
        assert!(names.contains(&"mcp__mcp__Read"));
        assert!(names.contains(&"mcp__mcp__Write"));
    }

    /// Test tool filtering with no restrictions (None patterns)
    #[test]
    fn test_integration_tool_filtering_no_restrictions() {
        let tools = vec![
            ToolDescription {
                name: "mcp__server__tool1".to_string(),
                description: "Tool 1".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__server__tool2".to_string(),
                description: "Tool 2".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__server__tool3".to_string(),
                description: "Tool 3".to_string(),
                ..Default::default()
            },
        ];

        // No patterns = all tools allowed
        let filtered = filter_tools_by_patterns(&tools, None);
        assert_eq!(filtered.len(), 3);

        let tools_json = build_tools_json(&tools, None);
        assert_eq!(tools_json.as_array().unwrap().len(), 3);
    }

    // ==========================================================================
    // Integration Tests: Error Handling
    // ==========================================================================

    /// Test that is_retryable_error correctly identifies retryable errors
    #[test]
    fn test_integration_error_handling_retryable() {
        // MCP operation errors are retryable
        let mcp_error = ServerError::McpOperation("Connection timeout".to_string());
        assert!(is_retryable_error(&mcp_error));

        // Tool call retry exhausted is retryable (at subtask level)
        let retry_error = ServerError::ToolCallRetryExhausted {
            tool_name: "test-tool".to_string(),
            attempts: 3,
            message: "Failed after retries".to_string(),
        };
        assert!(is_retryable_error(&retry_error));

        // Empty MCP content is retryable
        let empty_content = ServerError::McpEmptyContent;
        assert!(is_retryable_error(&empty_content));
    }

    /// Test that is_retryable_error correctly identifies non-retryable errors
    #[test]
    fn test_integration_error_handling_non_retryable() {
        // Cyclic dependency is not retryable
        let cyclic_error = ServerError::CyclicDependency;
        assert!(!is_retryable_error(&cyclic_error));

        // Empty plan is not retryable
        let empty_plan = ServerError::EmptyPlan;
        assert!(!is_retryable_error(&empty_plan));

        // Plan parse error is not retryable
        let parse_error = ServerError::PlanParseError("Invalid format".to_string());
        assert!(!is_retryable_error(&parse_error));
    }

    /// Test iteration trace records skill requests
    #[test]
    fn test_integration_iteration_trace_skill_request() {
        let mut iter_trace = IterationTrace::new(1);

        // Record a successful skill request
        iter_trace.set_skill_request("git-workflow".to_string(), true);

        assert_eq!(iter_trace.skill_requested, Some("git-workflow".to_string()));
        assert!(iter_trace.skill_loaded);

        // Test failed skill request
        let mut iter_trace2 = IterationTrace::new(2);
        iter_trace2.set_skill_request("nonexistent-skill".to_string(), false);

        assert_eq!(
            iter_trace2.skill_requested,
            Some("nonexistent-skill".to_string())
        );
        assert!(!iter_trace2.skill_loaded);
    }

    /// Test subtask trace records active skill
    #[test]
    fn test_integration_subtask_trace_active_skill() {
        let mut subtask_trace = SubtaskTrace::new(0, "Test task".to_string());

        // Initially no active skills
        assert!(subtask_trace.active_skills.is_empty());

        // Add active skill
        subtask_trace.add_active_skill("git-workflow".to_string());
        assert_eq!(
            subtask_trace.active_skills,
            vec!["git-workflow".to_string()]
        );

        // Verify summary includes skill info
        let summary = subtask_trace.summary();
        assert!(summary.contains("skill=git-workflow"));
    }

    /// Test complete workflow: planning -> skill detection -> tool filtering
    #[test]
    fn test_integration_complete_skill_workflow() {
        use std::path::PathBuf;

        use chrono::Utc;

        use crate::skills::SkillDetector;

        // Step 1: Simulate LLM response with skill request
        let llm_response = r#"
            I need to help with git operations.
            <use_skill>git-workflow</use_skill>
            Let me check the status first.
        "#;

        // Step 2: Detect skill request
        let detected_skill = SkillDetector::detect_first(llm_response);
        assert_eq!(detected_skill, Some("git-workflow".to_string()));

        // Step 3: Load skill (simulated)
        let loaded_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "git-workflow".to_string(),
                description: "Git workflow assistance".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("Bash(git:*) Read".to_string()),
                model: None,
                parameters: None,
            },
            content: "Git workflow instructions".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Step 4: Get allowed tools from skill
        let allowed_tools = loaded_skill.metadata.get_allowed_tools();
        assert_eq!(allowed_tools, vec!["Bash(git:*)", "Read"]);

        // Step 5: Filter tools
        let all_tools = vec![
            ToolDescription {
                name: "mcp__mcp__Bash(git:status)".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Bash(npm:install)".to_string(),
                description: "NPM install".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__mcp__Read".to_string(),
                description: "Read".to_string(),
                ..Default::default()
            },
        ];

        let filtered = filter_tools_by_patterns(&all_tools, Some(&allowed_tools));
        assert_eq!(filtered.len(), 2);

        // Step 6: Build context with active skill
        let subtask = SubTask::new(0, "Git task".to_string());
        let messages =
            build_context_for_react_sync(&subtask, &[], &all_tools, None, &[loaded_skill.clone()]);

        // Verify context includes skill (multi-skill format)
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            assert!(sys_msg.content().contains("Active Skills: git-workflow"));
        }

        // Step 7: Record in trace
        let mut subtask_trace = SubtaskTrace::new(0, "Git task".to_string());
        subtask_trace.set_active_skills(vec!["git-workflow".to_string()]);

        let mut iter_trace = IterationTrace::new(1);
        iter_trace.set_skill_request("git-workflow".to_string(), true);

        subtask_trace.add_iteration(iter_trace);

        assert!(subtask_trace.summary().contains("skill=git-workflow"));
    }

    /// Test skill detection with strip_tags for clean response
    #[test]
    fn test_integration_skill_detection_and_cleanup() {
        use crate::skills::SkillDetector;

        let response_with_skill = "I will use <use_skill>code-review</use_skill> to help you.";

        // Extract skill and clean response
        let (skills, cleaned) = SkillDetector::extract_and_clean(response_with_skill);

        assert_eq!(skills, vec!["code-review"]);
        assert_eq!(cleaned, "I will use  to help you.");
        assert!(!cleaned.contains("<use_skill>"));
    }

    // ==========================================================================
    // Multi-Skill Integration Tests
    // ==========================================================================

    /// Test multi-skill context building with merged tools
    #[test]
    fn test_integration_multi_skill_context() {
        use std::path::PathBuf;

        use chrono::Utc;

        let subtask = SubTask::new(0, "Review and document code".to_string());
        let previous_results: Vec<(usize, String)> = vec![];

        // Create tools
        let tools = vec![
            ToolDescription {
                name: "tool-a".to_string(),
                description: "Tool A".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool-b".to_string(),
                description: "Tool B".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool-c".to_string(),
                description: "Tool C".to_string(),
                ..Default::default()
            },
        ];

        // Create two skills with different allowed tools
        let skill_a = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "skill-a".to_string(),
                description: "Skill A".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("tool-a".to_string()),
                model: None,
                parameters: None,
            },
            content: "Skill A content".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        let skill_b = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "skill-b".to_string(),
                description: "Skill B".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("tool-b".to_string()),
                model: None,
                parameters: None,
            },
            content: "Skill B content".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        let active_skills = vec![skill_a, skill_b];

        // Build context with multiple active skills
        let messages =
            build_context_for_react_sync(&subtask, &previous_results, &tools, None, &active_skills);

        // Verify multi-skill format in system prompt
        if let ChatCompletionRequestMessage::System(sys_msg) = &messages[0] {
            let content = sys_msg.content();
            // Should show both skills
            assert!(content.contains("Active Skills: skill-a, skill-b"));
            // Should contain both skill contents
            assert!(content.contains("Skill A content"));
            assert!(content.contains("Skill B content"));
        } else {
            panic!("Expected system message");
        }
    }

    /// Test multi-skill tool filtering (merged allowed_tools)
    #[test]
    fn test_integration_multi_skill_tool_filtering() {
        use std::path::PathBuf;

        use chrono::Utc;

        let tools = vec![
            ToolDescription {
                name: "tool-a".to_string(),
                description: "Tool A".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool-b".to_string(),
                description: "Tool B".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool-c".to_string(),
                description: "Tool C".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool-shared".to_string(),
                description: "Shared Tool".to_string(),
                ..Default::default()
            },
        ];

        // Skill A allows tool-a and tool-shared
        let skill_a = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "skill-a".to_string(),
                description: "Skill A".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("tool-a tool-shared".to_string()),
                model: None,
                parameters: None,
            },
            content: "Content A".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Skill B allows tool-b and tool-shared
        let skill_b = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "skill-b".to_string(),
                description: "Skill B".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("tool-b tool-shared".to_string()),
                model: None,
                parameters: None,
            },
            content: "Content B".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        let active_skills = vec![skill_a, skill_b];

        // Filter with multiple skills - should get merged allowed_tools
        let filtered = filter_tools_by_skills(&tools, None, &active_skills);

        // Should have tool-a, tool-b, tool-shared (merged, deduplicated)
        assert_eq!(filtered.len(), 3);
        let names: Vec<&str> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"tool-a"));
        assert!(names.contains(&"tool-b"));
        assert!(names.contains(&"tool-shared"));
        // tool-c should NOT be included
        assert!(!names.contains(&"tool-c"));
    }

    /// Test multi-skill tracing with set_active_skills
    #[test]
    fn test_integration_multi_skill_tracing() {
        let mut subtask_trace = SubtaskTrace::new(0, "Multi-skill task".to_string());

        // Initially empty
        assert!(subtask_trace.active_skills.is_empty());

        // Set multiple skills at once
        subtask_trace.set_active_skills(vec![
            "skill-a".to_string(),
            "skill-b".to_string(),
            "skill-c".to_string(),
        ]);

        assert_eq!(subtask_trace.active_skills.len(), 3);
        assert_eq!(
            subtask_trace.active_skills,
            vec!["skill-a", "skill-b", "skill-c"]
        );

        // Verify summary shows multi-skill format
        let summary = subtask_trace.summary();
        assert!(summary.contains("skills=[skill-a, skill-b, skill-c]"));
    }

    // ==========================================================================
    // Unit Tests: filter_tools_by_skills
    // ==========================================================================

    /// Test Phase 1 filtering: tools covered by skills are hidden
    #[test]
    fn test_filter_tools_by_skills_phase1_basic() {
        let tools = vec![
            ToolDescription {
                name: "mcp__calc__sum".to_string(),
                description: "Sum numbers".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__calc__sub".to_string(),
                description: "Subtract numbers".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__search__query".to_string(),
                description: "Search query".to_string(),
                ..Default::default()
            },
        ];

        let skills = vec![SkillSummary {
            name: "calculator".to_string(),
            description: "Calculator operations".to_string(),
            allowed_tools: vec!["mcp__calc__sum".to_string(), "mcp__calc__sub".to_string()],
            parameters: None,
        }];

        // Phase 1: no active skill, with skill summaries
        let filtered = filter_tools_by_skills(&tools, Some(&skills), &[]);

        // Only search tool should remain (calc tools are covered by skill)
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "mcp__search__query");
    }

    /// Test Phase 1 filtering with multiple skills
    #[test]
    fn test_filter_tools_by_skills_phase1_multiple_skills() {
        let tools = vec![
            ToolDescription {
                name: "mcp__calc__sum".to_string(),
                description: "Sum".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__search__query".to_string(),
                description: "Search".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__git__status".to_string(),
                description: "Git status".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__generic__tool".to_string(),
                description: "Generic tool".to_string(),
                ..Default::default()
            },
        ];

        let skills = vec![
            SkillSummary {
                name: "calculator".to_string(),
                description: "Calculator".to_string(),
                allowed_tools: vec!["mcp__calc__sum".to_string()],
                parameters: None,
            },
            SkillSummary {
                name: "search".to_string(),
                description: "Search".to_string(),
                allowed_tools: vec!["mcp__search__query".to_string()],
                parameters: None,
            },
        ];

        let filtered = filter_tools_by_skills(&tools, Some(&skills), &[]);

        // Only git and generic tools should remain
        assert_eq!(filtered.len(), 2);
        let names: Vec<&str> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"mcp__git__status"));
        assert!(names.contains(&"mcp__generic__tool"));
    }

    /// Test Phase 1 filtering with no skill summaries
    #[test]
    fn test_filter_tools_by_skills_phase1_no_skills() {
        let tools = vec![
            ToolDescription {
                name: "tool1".to_string(),
                description: "Tool 1".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool2".to_string(),
                description: "Tool 2".to_string(),
                ..Default::default()
            },
        ];

        // No skills = all tools shown
        let filtered = filter_tools_by_skills(&tools, None, &[]);
        assert_eq!(filtered.len(), 2);

        // Empty skills = all tools shown
        let empty_skills: Vec<SkillSummary> = vec![];
        let filtered = filter_tools_by_skills(&tools, Some(&empty_skills), &[]);
        assert_eq!(filtered.len(), 2);
    }

    /// Test Phase 1 filtering with skills having no allowed_tools
    #[test]
    fn test_filter_tools_by_skills_phase1_skills_without_allowed_tools() {
        let tools = vec![
            ToolDescription {
                name: "tool1".to_string(),
                description: "Tool 1".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool2".to_string(),
                description: "Tool 2".to_string(),
                ..Default::default()
            },
        ];

        // Skill exists but has no allowed_tools = all tools shown
        let skills = vec![SkillSummary {
            name: "generic-skill".to_string(),
            description: "A skill without tool restrictions".to_string(),
            allowed_tools: vec![],
            parameters: None,
        }];

        let filtered = filter_tools_by_skills(&tools, Some(&skills), &[]);
        assert_eq!(filtered.len(), 2);
    }

    /// Test Phase 2 filtering: only skill's allowed tools are shown
    #[test]
    fn test_filter_tools_by_skills_phase2_basic() {
        use std::path::PathBuf;

        use chrono::Utc;

        let tools = vec![
            ToolDescription {
                name: "mcp__calc__sum".to_string(),
                description: "Sum".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__calc__sub".to_string(),
                description: "Subtract".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__search__query".to_string(),
                description: "Search".to_string(),
                ..Default::default()
            },
        ];

        let active_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "calculator".to_string(),
                description: "Calculator".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("mcp__calc__sum, mcp__calc__sub".to_string()),
                model: None,
                parameters: None,
            },
            content: "Calculator instructions".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Phase 2: active skill present
        let filtered = filter_tools_by_skills(&tools, None, &[active_skill]);

        // Only calc tools should be shown
        assert_eq!(filtered.len(), 2);
        let names: Vec<&str> = filtered.iter().map(|t| t.name.as_str()).collect();
        assert!(names.contains(&"mcp__calc__sum"));
        assert!(names.contains(&"mcp__calc__sub"));
        assert!(!names.contains(&"mcp__search__query"));
    }

    /// Test Phase 2 filtering: skill without allowed_tools shows all tools
    #[test]
    fn test_filter_tools_by_skills_phase2_no_restrictions() {
        use std::path::PathBuf;

        use chrono::Utc;

        let tools = vec![
            ToolDescription {
                name: "tool1".to_string(),
                description: "Tool 1".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "tool2".to_string(),
                description: "Tool 2".to_string(),
                ..Default::default()
            },
        ];

        let active_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "generic-skill".to_string(),
                description: "A skill without tool restrictions".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: None, // No restrictions
                model: None,
                parameters: None,
            },
            content: "Skill content".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Phase 2 with no restrictions = all tools shown
        let filtered = filter_tools_by_skills(&tools, None, &[active_skill]);
        assert_eq!(filtered.len(), 2);
    }

    /// Test Phase 2 takes precedence over Phase 1
    #[test]
    fn test_filter_tools_by_skills_phase2_overrides_phase1() {
        use std::path::PathBuf;

        use chrono::Utc;

        let tools = vec![
            ToolDescription {
                name: "mcp__calc__sum".to_string(),
                description: "Sum".to_string(),
                ..Default::default()
            },
            ToolDescription {
                name: "mcp__search__query".to_string(),
                description: "Search".to_string(),
                ..Default::default()
            },
        ];

        // Even if skill summaries say to hide calc tools...
        let skills = vec![SkillSummary {
            name: "calculator".to_string(),
            description: "Calculator".to_string(),
            allowed_tools: vec!["mcp__calc__sum".to_string()],
            parameters: None,
        }];

        // ...when active skill is present, Phase 2 logic applies
        let active_skill = LoadedSkill {
            metadata: crate::skills::SkillMetadata {
                name: "calculator".to_string(),
                description: "Calculator".to_string(),
                license: None,
                compatibility: None,
                metadata: None,
                allowed_tools: Some("mcp__calc__sum".to_string()),
                model: None,
                parameters: None,
            },
            content: "Calculator".to_string(),
            raw_content: String::new(),
            skill_dir: PathBuf::new(),
            file_path: String::new(),
            enabled: true,
            loaded_at: Utc::now(),
            scripts: Vec::new(),
        };

        // Phase 2: only show skill's allowed tools
        let filtered = filter_tools_by_skills(&tools, Some(&skills), &[active_skill]);

        // Only calc tool should be shown (Phase 2 filtering)
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "mcp__calc__sum");
    }

    /// Test filter_tools_by_skills with empty tools list
    #[test]
    fn test_filter_tools_by_skills_empty_tools() {
        let tools: Vec<ToolDescription> = vec![];

        let skills = vec![SkillSummary {
            name: "skill".to_string(),
            description: "A skill".to_string(),
            allowed_tools: vec!["some_tool".to_string()],
            parameters: None,
        }];

        let filtered = filter_tools_by_skills(&tools, Some(&skills), &[]);
        assert!(filtered.is_empty());
    }

    // ============================================================================
    // Template Variable Replacement Tests
    // ============================================================================

    #[test]
    fn test_apply_template_variables_string() {
        let mut vars = serde_json::Map::new();
        vars.insert("name".to_string(), serde_json::json!("John"));
        vars.insert("age".to_string(), serde_json::json!(30));

        let content = "Hello, {{name}}! You are {{age}} years old.";
        let result = apply_template_variables(content, &vars);

        assert_eq!(result, "Hello, John! You are 30 years old.");
    }

    #[test]
    fn test_apply_template_variables_multiple_same_var() {
        let mut vars = serde_json::Map::new();
        vars.insert("item".to_string(), serde_json::json!("apple"));

        let content = "Buy {{item}}, eat {{item}}, enjoy {{item}}.";
        let result = apply_template_variables(content, &vars);

        assert_eq!(result, "Buy apple, eat apple, enjoy apple.");
    }

    #[test]
    fn test_apply_template_variables_missing_var() {
        let vars = serde_json::Map::new();

        let content = "Hello, {{name}}!";
        let result = apply_template_variables(content, &vars);

        // Missing variables remain unchanged
        assert_eq!(result, "Hello, {{name}}!");
    }

    #[test]
    fn test_apply_template_variables_bool_and_null() {
        let mut vars = serde_json::Map::new();
        vars.insert("active".to_string(), serde_json::json!(true));
        vars.insert("empty".to_string(), serde_json::json!(null));

        let content = "Active: {{active}}, Empty: {{empty}}";
        let result = apply_template_variables(content, &vars);

        assert_eq!(result, "Active: true, Empty: null");
    }

    #[test]
    fn test_apply_template_variables_array_object() {
        let mut vars = serde_json::Map::new();
        vars.insert("list".to_string(), serde_json::json!([1, 2, 3]));
        vars.insert("obj".to_string(), serde_json::json!({"key": "value"}));

        let content = "List: {{list}}, Obj: {{obj}}";
        let result = apply_template_variables(content, &vars);

        assert!(result.contains("[1,2,3]"));
        assert!(result.contains(r#"{"key":"value"}"#));
    }

    #[test]
    fn test_apply_template_variables_empty_content() {
        let mut vars = serde_json::Map::new();
        vars.insert("name".to_string(), serde_json::json!("John"));

        let content = "";
        let result = apply_template_variables(content, &vars);

        assert_eq!(result, "");
    }

    // ============================================================================
    // JSON/YAML Parsing Tests
    // ============================================================================

    #[test]
    fn test_parse_as_json_valid() {
        let content = r#"{"name": "test", "value": 42}"#;
        let result = parse_as_json(content, "config.json").unwrap();

        assert!(result.contains("config.json"));
        assert!(result.contains("parsed as JSON"));
        assert!(result.contains("\"name\": \"test\""));
        assert!(result.contains("\"value\": 42"));
    }

    #[test]
    fn test_parse_as_json_invalid() {
        let content = "not valid json {";
        let result = parse_as_json(content, "bad.json");

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Failed to parse"));
        }
    }

    #[test]
    fn test_parse_as_yaml_valid() {
        let content = "name: test\nvalue: 42";
        let result = parse_as_yaml(content, "config.yaml").unwrap();

        assert!(result.contains("config.yaml"));
        assert!(result.contains("parsed as YAML"));
        assert!(result.contains("name:"));
        assert!(result.contains("test"));
    }

    #[test]
    fn test_parse_as_yaml_invalid() {
        let content = ":\n  invalid: [unclosed";
        let result = parse_as_yaml(content, "bad.yaml");

        assert!(result.is_err());
        if let Err(e) = result {
            assert!(e.to_string().contains("Failed to parse"));
        }
    }

    #[test]
    fn test_format_as_markdown() {
        let content = "# Title\n\nSome content here.";
        let result = format_as_markdown(content, "doc.md");

        assert!(result.contains("doc.md"));
        assert!(result.contains("loaded as Markdown"));
        assert!(result.contains("# Title"));
        assert!(result.contains("Some content here."));
    }

    // ============================================================================
    // Internal Tool Name Tests
    // ============================================================================

    #[test]
    fn test_internal_tool_names() {
        assert_eq!(
            internal_tool_name(SKILL_RUN_SCRIPT_TOOL),
            "internal__skill_run_script"
        );
        assert_eq!(
            internal_tool_name(SKILL_LOAD_ASSET_TOOL),
            "internal__skill_load_asset"
        );
    }

    #[test]
    fn test_is_internal_tool() {
        assert!(is_internal_tool("internal__skill_run_script"));
        assert!(is_internal_tool("internal__skill_load_asset"));
        assert!(is_internal_tool("internal__any_tool"));
        assert!(!is_internal_tool("mcp__server__tool"));
        assert!(!is_internal_tool("some_tool"));
    }

    #[test]
    fn test_parse_internal_tool_name() {
        assert_eq!(
            parse_internal_tool_name("internal__skill_run_script"),
            Some("skill_run_script")
        );
        assert_eq!(
            parse_internal_tool_name("internal__skill_load_asset"),
            Some("skill_load_asset")
        );
        assert_eq!(parse_internal_tool_name("mcp__server__tool"), None);
    }
}
