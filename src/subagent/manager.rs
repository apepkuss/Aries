//! Sub-Agent 管理器
//!
//! 此模块实现 SubAgentManager，负责 Sub-Agent 的生命周期管理。

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use http::HeaderMap;
use tokio::sync::{RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::{
    SubAgentContext, SubAgentExecutor,
    config::{SubAgentSpawnConfig, SubAgentSystemConfig},
    types::{SubAgent, SubAgentId, SubAgentResult, SubAgentState},
};
use crate::{
    app::AppState,
    chat::{emitter::EventEmitter, planner::ToolDescription},
    error::{ServerError, ServerResult},
    server::TargetServerInfo,
};

// ============================================================================
// SubAgentManager
// ============================================================================

/// Sub-Agent 管理器
///
/// 负责管理所有 Sub-Agent 的生命周期，包括创建、查询、取消等操作。
/// 使用 `RwLock<HashMap>` 存储 Sub-Agent 状态，支持并发访问。
///
/// # Example
///
/// ```rust,ignore
/// use moss::subagent::{SubAgentManager, SubAgentSystemConfig};
///
/// let config = SubAgentSystemConfig::default_enabled();
/// let manager = SubAgentManager::new(config);
///
/// // Spawn a new Sub-Agent
/// let id = manager.spawn(
///     "DataAnalyst",
///     "You are a data analyst.",
///     "Analyze the sales data",
///     None,
///     None,
/// ).await?;
///
/// // Get Sub-Agent info
/// let agent = manager.get(&id).await?;
/// println!("State: {}", agent.state);
/// ```
pub struct SubAgentManager {
    /// 系统配置
    config: SubAgentSystemConfig,
    /// Sub-Agent 存储
    agents: RwLock<HashMap<SubAgentId, SubAgent>>,
    /// 当前运行中的 Sub-Agent 数量
    running_count: AtomicUsize,
    /// 取消令牌映射（用于取消正在执行的 Sub-Agent）
    cancel_tokens: RwLock<HashMap<SubAgentId, CancellationToken>>,
    /// 并发控制信号量
    concurrency_semaphore: Arc<Semaphore>,
    /// 全局 Token 使用量（所有 Sub-Agent 总计）
    global_prompt_tokens: AtomicU64,
    /// 全局完成 Token 使用量
    global_completion_tokens: AtomicU64,
    /// Sub-Agent 启动时间映射（用于超时检测）
    start_times: RwLock<HashMap<SubAgentId, Instant>>,
}

impl SubAgentManager {
    /// 创建新的 SubAgentManager
    pub fn new(config: SubAgentSystemConfig) -> Self {
        let max_concurrent = config.max_concurrent;
        Self {
            config,
            agents: RwLock::new(HashMap::new()),
            running_count: AtomicUsize::new(0),
            cancel_tokens: RwLock::new(HashMap::new()),
            concurrency_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            global_prompt_tokens: AtomicU64::new(0),
            global_completion_tokens: AtomicU64::new(0),
            start_times: RwLock::new(HashMap::new()),
        }
    }

    /// 检查 Sub-Agent 功能是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// 获取配置引用
    pub fn config(&self) -> &SubAgentSystemConfig {
        &self.config
    }

    /// 创建新的 Sub-Agent
    ///
    /// # Arguments
    ///
    /// * `name` - Sub-Agent 名称
    /// * `system_prompt` - 系统提示词
    /// * `task` - 任务描述
    /// * `spawn_config` - 创建配置（可选）
    /// * `parent_id` - 父 Sub-Agent ID（用于嵌套场景）
    ///
    /// # Returns
    ///
    /// 成功返回新创建的 SubAgentId，失败返回错误
    pub async fn spawn(
        &self,
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        task: impl Into<String>,
        spawn_config: Option<SubAgentSpawnConfig>,
        parent_id: Option<SubAgentId>,
    ) -> ServerResult<SubAgentId> {
        self.spawn_with_subtask_id(name, system_prompt, task, spawn_config, parent_id, None)
            .await
    }

    /// 创建新的 Sub-Agent，并设置子任务 ID
    ///
    /// # Arguments
    ///
    /// * `name` - Sub-Agent 名称
    /// * `system_prompt` - 系统提示词
    /// * `task` - 任务描述
    /// * `spawn_config` - 创建配置（可选）
    /// * `parent_id` - 父 Sub-Agent ID（用于嵌套场景）
    /// * `subtask_id` - 子任务 ID（1-based，用于 HITL 显示标识）
    ///
    /// # Returns
    ///
    /// 成功返回新创建的 SubAgentId，失败返回错误
    pub async fn spawn_with_subtask_id(
        &self,
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        task: impl Into<String>,
        spawn_config: Option<SubAgentSpawnConfig>,
        parent_id: Option<SubAgentId>,
        subtask_id: Option<usize>,
    ) -> ServerResult<SubAgentId> {
        // 检查是否启用
        if !self.config.enabled {
            return Err(ServerError::SubAgentDisabled);
        }

        // 检查并发限制
        let current_running = self.running_count.load(Ordering::SeqCst);
        if current_running >= self.config.max_concurrent {
            return Err(ServerError::SubAgentLimitExceeded {
                max: self.config.max_concurrent,
            });
        }

        // 计算嵌套深度
        let (depth, parent_depth) = if let Some(ref pid) = parent_id {
            let agents = self.agents.read().await;
            if let Some(parent) = agents.get(pid) {
                (parent.depth + 1, parent.depth)
            } else {
                return Err(ServerError::SubAgentNotFound(pid.to_string()));
            }
        } else {
            (0, 0)
        };

        // 检查嵌套深度限制
        if depth > self.config.max_nesting_depth {
            return Err(ServerError::SubAgentMaxDepthExceeded {
                max_depth: self.config.max_nesting_depth,
                current_depth: depth,
            });
        }

        // 创建 Sub-Agent
        let mut agent = SubAgent::new(name, system_prompt, task);

        // 设置父 Agent
        if let Some(pid) = parent_id {
            agent = agent.with_parent(pid, parent_depth);
        }

        // 设置子任务 ID（用于 HITL 显示标识）
        if let Some(id) = subtask_id {
            agent = agent.with_subtask_id(id);
        }

        // 应用工具访问配置
        let spawn_config = spawn_config.unwrap_or_default();
        if let Some(allowed) = spawn_config.tool_access.allowed_tools {
            agent = agent.with_allowed_tools(allowed);
        } else if !self.config.allow_full_tool_access {
            // 使用系统默认允许的工具
            agent = agent.with_allowed_tools(self.config.default_allowed_tools.clone());
        }

        // 合并禁止的工具列表
        let mut blocked = self.config.default_blocked_tools.clone();
        blocked.extend(spawn_config.tool_access.blocked_tools);
        agent = agent.with_blocked_tools(blocked);

        let id = agent.id.clone();

        // 存储 Sub-Agent
        {
            let mut agents = self.agents.write().await;
            agents.insert(id.clone(), agent);
        }

        // 创建取消令牌
        {
            let mut tokens = self.cancel_tokens.write().await;
            tokens.insert(id.clone(), CancellationToken::new());
        }

        Ok(id)
    }

    /// 获取 Sub-Agent 信息
    pub async fn get(&self, id: &SubAgentId) -> ServerResult<SubAgent> {
        let agents = self.agents.read().await;
        agents
            .get(id)
            .cloned()
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))
    }

    /// 获取 Sub-Agent 状态
    pub async fn get_state(&self, id: &SubAgentId) -> ServerResult<SubAgentState> {
        let agents = self.agents.read().await;
        agents
            .get(id)
            .map(|a| a.state)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))
    }

    /// 获取 Sub-Agent 结果
    ///
    /// 仅当 Sub-Agent 处于终态时返回结果
    pub async fn get_result(&self, id: &SubAgentId) -> ServerResult<Option<SubAgentResult>> {
        let agents = self.agents.read().await;
        let agent = agents
            .get(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        Ok(agent.result.clone())
    }

    /// 列出所有 Sub-Agent
    pub async fn list(&self) -> Vec<SubAgent> {
        let agents = self.agents.read().await;
        agents.values().cloned().collect()
    }

    /// 列出指定状态的 Sub-Agent
    pub async fn list_by_state(&self, state: SubAgentState) -> Vec<SubAgent> {
        let agents = self.agents.read().await;
        agents
            .values()
            .filter(|a| a.state == state)
            .cloned()
            .collect()
    }

    /// 列出指定父 Agent 的子 Agent
    pub async fn list_children(&self, parent_id: &SubAgentId) -> Vec<SubAgent> {
        let agents = self.agents.read().await;
        agents
            .values()
            .filter(|a| a.parent_id.as_ref() == Some(parent_id))
            .cloned()
            .collect()
    }

    /// 取消 Sub-Agent
    ///
    /// 如果 Sub-Agent 正在运行，会触发取消信号
    pub async fn cancel(&self, id: &SubAgentId) -> ServerResult<()> {
        // 触发取消信号
        {
            let tokens = self.cancel_tokens.read().await;
            if let Some(token) = tokens.get(id) {
                token.cancel();
            }
        }

        // 更新状态
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        if agent.state.is_terminal() {
            return Err(ServerError::SubAgentAlreadyTerminal {
                id: id.to_string(),
                state: agent.state.to_string(),
            });
        }

        if agent.cancel() {
            // 如果之前是 Running 状态，减少计数
            if agent.state == SubAgentState::Cancelled {
                self.running_count.fetch_sub(1, Ordering::SeqCst);
            }
        }

        Ok(())
    }

    /// 标记 Sub-Agent 为开始运行
    ///
    /// 内部方法，由 Executor 调用
    #[allow(dead_code)]
    pub(crate) async fn mark_started(&self, id: &SubAgentId) -> ServerResult<()> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        if agent.start() {
            self.running_count.fetch_add(1, Ordering::SeqCst);
            // 记录启动时间
            {
                let mut start_times = self.start_times.write().await;
                start_times.insert(id.clone(), Instant::now());
            }
            Ok(())
        } else {
            Err(ServerError::SubAgentAlreadyTerminal {
                id: id.to_string(),
                state: agent.state.to_string(),
            })
        }
    }

    /// 标记 Sub-Agent 为完成
    ///
    /// 内部方法，由 Executor 调用
    #[allow(dead_code)]
    pub(crate) async fn mark_completed(
        &self,
        id: &SubAgentId,
        result: SubAgentResult,
    ) -> ServerResult<()> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        let was_running = agent.state == SubAgentState::Running;
        if agent.complete(result) {
            if was_running {
                self.running_count.fetch_sub(1, Ordering::SeqCst);
            }
            // 清理启动时间记录
            {
                let mut start_times = self.start_times.write().await;
                start_times.remove(id);
            }
            Ok(())
        } else {
            Err(ServerError::SubAgentAlreadyTerminal {
                id: id.to_string(),
                state: agent.state.to_string(),
            })
        }
    }

    /// 标记 Sub-Agent 为失败
    ///
    /// 内部方法，由 Executor 调用
    #[allow(dead_code)]
    pub(crate) async fn mark_failed(&self, id: &SubAgentId, error: String) -> ServerResult<()> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        let was_running = agent.state == SubAgentState::Running;
        if agent.fail(error) {
            if was_running {
                self.running_count.fetch_sub(1, Ordering::SeqCst);
            }
            // 清理启动时间记录
            {
                let mut start_times = self.start_times.write().await;
                start_times.remove(id);
            }
            Ok(())
        } else {
            Err(ServerError::SubAgentAlreadyTerminal {
                id: id.to_string(),
                state: agent.state.to_string(),
            })
        }
    }

    /// 获取取消令牌
    ///
    /// 内部方法，由 Executor 使用
    #[allow(dead_code)]
    pub(crate) async fn get_cancel_token(&self, id: &SubAgentId) -> Option<CancellationToken> {
        let tokens = self.cancel_tokens.read().await;
        tokens.get(id).cloned()
    }

    /// 更新 Sub-Agent 指标
    ///
    /// 内部方法，由 Executor 调用
    #[allow(dead_code)]
    pub(crate) async fn update_metrics(
        &self,
        id: &SubAgentId,
        updater: impl FnOnce(&mut super::types::SubAgentMetrics),
    ) -> ServerResult<()> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        updater(&mut agent.metrics);
        Ok(())
    }

    /// 获取当前运行中的 Sub-Agent 数量
    pub fn running_count(&self) -> usize {
        self.running_count.load(Ordering::SeqCst)
    }

    /// 获取统计信息
    pub async fn stats(&self) -> SubAgentStats {
        let agents = self.agents.read().await;
        let mut stats = SubAgentStats::default();

        for agent in agents.values() {
            stats.total += 1;
            match agent.state {
                SubAgentState::Pending => stats.pending += 1,
                SubAgentState::Running => stats.running += 1,
                SubAgentState::Completed => stats.completed += 1,
                SubAgentState::Failed => stats.failed += 1,
                SubAgentState::Cancelled => stats.cancelled += 1,
            }
            stats.total_prompt_tokens += agent.metrics.prompt_tokens;
            stats.total_completion_tokens += agent.metrics.completion_tokens;
        }

        // 添加全局 token 统计
        let (global_prompt, global_completion) = self.global_token_usage();
        stats.global_prompt_tokens = global_prompt;
        stats.global_completion_tokens = global_completion;
        stats.max_total_tokens = self.config.max_total_tokens;
        stats.token_limit_exceeded = self.is_token_limit_exceeded();

        stats
    }

    /// 清理已完成的 Sub-Agent（释放内存）
    ///
    /// 返回清理的数量
    pub async fn cleanup_completed(&self) -> usize {
        let mut agents = self.agents.write().await;
        let mut tokens = self.cancel_tokens.write().await;
        let mut start_times = self.start_times.write().await;

        let to_remove: Vec<SubAgentId> = agents
            .iter()
            .filter(|(_, a)| a.state.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            agents.remove(&id);
            tokens.remove(&id);
            start_times.remove(&id);
        }

        count
    }

    // ============================================================================
    // Resource Control Methods
    // ============================================================================

    /// 获取全局 Token 使用量
    pub fn global_token_usage(&self) -> (u64, u64) {
        (
            self.global_prompt_tokens.load(Ordering::SeqCst),
            self.global_completion_tokens.load(Ordering::SeqCst),
        )
    }

    /// 获取全局 Token 总量
    pub fn global_total_tokens(&self) -> u64 {
        self.global_prompt_tokens.load(Ordering::SeqCst)
            + self.global_completion_tokens.load(Ordering::SeqCst)
    }

    /// 检查是否超出全局 Token 限制
    ///
    /// 如果 `max_total_tokens` 为 0，则不限制
    pub fn is_token_limit_exceeded(&self) -> bool {
        let max = self.config.max_total_tokens;
        if max == 0 {
            return false;
        }
        self.global_total_tokens() >= max
    }

    /// 添加全局 Token 使用量
    ///
    /// 由 Executor 调用以报告 Token 使用
    pub(crate) fn add_global_tokens(&self, prompt_tokens: u64, completion_tokens: u64) {
        self.global_prompt_tokens
            .fetch_add(prompt_tokens, Ordering::SeqCst);
        self.global_completion_tokens
            .fetch_add(completion_tokens, Ordering::SeqCst);
    }

    /// 检查并取消超时的 Sub-Agent
    ///
    /// 返回取消的数量
    pub async fn check_and_cancel_timed_out(&self) -> usize {
        let timeout = Duration::from_secs(self.config.default_timeout_secs);
        let now = Instant::now();
        let mut timed_out_ids = Vec::new();

        // 收集超时的 Sub-Agent ID
        {
            let start_times = self.start_times.read().await;
            for (id, start_time) in start_times.iter() {
                if now.duration_since(*start_time) > timeout {
                    timed_out_ids.push(id.clone());
                }
            }
        }

        // 取消超时的 Sub-Agent
        let mut cancelled_count = 0;
        for id in timed_out_ids {
            debug!(subagent_id = %id, "Sub-Agent timed out, cancelling");
            if self.cancel(&id).await.is_ok() {
                cancelled_count += 1;
            }
        }

        if cancelled_count > 0 {
            info!(count = cancelled_count, "Cancelled timed out Sub-Agents");
        }

        cancelled_count
    }

    /// 启动超时监控任务
    ///
    /// 定期检查并取消超时的 Sub-Agent
    pub fn start_timeout_monitor(self: &Arc<Self>, check_interval: Duration) {
        let manager = Arc::clone(self);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(check_interval).await;

                // 检查是否还有运行中的 Sub-Agent
                if manager.running_count() == 0 {
                    continue;
                }

                let cancelled = manager.check_and_cancel_timed_out().await;
                if cancelled > 0 {
                    debug!(count = cancelled, "Timeout monitor cancelled Sub-Agents");
                }
            }
        });
    }

    /// 重置全局 Token 计数器
    ///
    /// 仅用于测试或会话重置
    #[allow(dead_code)]
    pub(crate) fn reset_global_tokens(&self) {
        self.global_prompt_tokens.store(0, Ordering::SeqCst);
        self.global_completion_tokens.store(0, Ordering::SeqCst);
    }

    // ============================================================================
    // Graceful Exit Methods
    // ============================================================================

    /// Cancel a Sub-Agent with a grace period for completion.
    ///
    /// This method provides graceful cancellation:
    /// 1. Sends a cancellation signal to the Sub-Agent
    /// 2. Waits for the grace period to allow completion
    /// 3. If not completed, forcefully terminates and returns partial result
    ///
    /// # Arguments
    ///
    /// * `id` - The Sub-Agent ID to cancel
    /// * `grace_period` - Time to wait for graceful completion
    ///
    /// # Returns
    ///
    /// Returns the result if the Sub-Agent completed within the grace period,
    /// or a partial result if forcefully terminated.
    pub async fn cancel_with_grace_period(
        &self,
        id: &SubAgentId,
        grace_period: Duration,
    ) -> ServerResult<Option<SubAgentResult>> {
        info!(subagent_id = %id, grace_period_ms = grace_period.as_millis(), "Cancelling Sub-Agent with grace period");

        // 1. Send cancellation signal
        self.cancel(id).await?;

        // 2. Wait for graceful completion during grace period
        let deadline = Instant::now() + grace_period;
        let poll_interval = Duration::from_millis(100);

        loop {
            // Check if the Sub-Agent has completed
            if let Some(result) = self.get_result(id).await? {
                info!(subagent_id = %id, "Sub-Agent completed gracefully during grace period");
                return Ok(Some(result));
            }

            // Check state
            let state = self.get_state(id).await?;
            if state.is_terminal() {
                // Already terminated, try to get result one more time
                return self.get_result(id).await;
            }

            // Check deadline
            if Instant::now() >= deadline {
                break;
            }

            // Wait before next poll
            tokio::time::sleep(poll_interval).await;
        }

        // 3. Grace period expired, force terminate
        warn!(subagent_id = %id, "Grace period expired, forcing termination");
        self.force_terminate(id).await?;

        // 4. Return partial result if available
        self.get_partial_result(id).await
    }

    /// Forcefully terminate a Sub-Agent.
    ///
    /// This method forcefully stops the Sub-Agent without waiting for completion.
    /// It marks the Sub-Agent as failed with a termination message.
    pub async fn force_terminate(&self, id: &SubAgentId) -> ServerResult<()> {
        let mut agents = self.agents.write().await;
        let agent = agents
            .get_mut(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        // If already terminal, nothing to do
        if agent.state.is_terminal() {
            return Ok(());
        }

        let was_running = agent.state == SubAgentState::Running;

        // Mark as failed with termination reason
        agent.fail("Forcefully terminated due to timeout".to_string());

        if was_running {
            self.running_count.fetch_sub(1, Ordering::SeqCst);
        }

        // Clean up
        {
            let mut start_times = self.start_times.write().await;
            start_times.remove(id);
        }

        warn!(subagent_id = %id, "Sub-Agent forcefully terminated");
        Ok(())
    }

    /// Get partial result from a Sub-Agent.
    ///
    /// This method attempts to retrieve any partial output that the Sub-Agent
    /// may have produced before termination. Returns None if no partial result
    /// is available.
    pub async fn get_partial_result(
        &self,
        id: &SubAgentId,
    ) -> ServerResult<Option<SubAgentResult>> {
        let agents = self.agents.read().await;
        let agent = agents
            .get(id)
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        // If there's already a result, return it
        if let Some(result) = &agent.result {
            return Ok(Some(result.clone()));
        }

        // If no result but agent has accumulated some output in context,
        // create a partial result
        if agent.state.is_terminal() {
            // Create a partial result from available information
            let partial_output = format!(
                "[Partial result: Sub-Agent '{}' was terminated. Iterations completed: {}]",
                agent.name, agent.metrics.total_iterations
            );

            Ok(Some(SubAgentResult {
                output: partial_output,
                artifacts: Vec::new(),
                metrics: agent.metrics.clone(),
                error: Some("Terminated before completion".to_string()),
            }))
        } else {
            Ok(None)
        }
    }

    /// 异步启动 Sub-Agent 执行
    ///
    /// 此方法会在后台 spawn 一个任务来执行 Sub-Agent。
    /// 使用 Semaphore 控制并发数量。
    ///
    /// # Arguments
    ///
    /// * `id` - Sub-Agent ID
    /// * `state` - 应用状态
    /// * `chat_server` - 聊天服务器信息
    /// * `headers` - HTTP 头信息
    /// * `available_tools` - 可用工具列表
    /// * `model` - 模型名称
    /// * `emitter` - 事件发射器
    ///
    /// # Returns
    ///
    /// 成功返回 `Ok(())`，如果 Sub-Agent 不存在或状态不对返回错误
    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        self: &Arc<Self>,
        id: SubAgentId,
        state: Arc<AppState>,
        chat_server: TargetServerInfo,
        headers: HeaderMap,
        available_tools: Vec<ToolDescription>,
        model: String,
        emitter: Arc<dyn EventEmitter>,
    ) -> ServerResult<()> {
        // 验证 Sub-Agent 存在且状态为 Pending
        {
            let agents = self.agents.read().await;
            let agent = agents
                .get(&id)
                .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

            if agent.state != SubAgentState::Pending {
                return Err(ServerError::SubAgentAlreadyTerminal {
                    id: id.to_string(),
                    state: agent.state.to_string(),
                });
            }
        }

        // 获取取消令牌
        let cancel_token = self
            .get_cancel_token(&id)
            .await
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        // 获取 Sub-Agent 配置
        let timeout = Duration::from_secs(self.config.default_timeout_secs);
        let max_iterations = self.config.default_max_iterations;

        // 克隆必要的引用
        let manager = Arc::clone(self);
        let semaphore = Arc::clone(&self.concurrency_semaphore);
        let id_clone = id.clone();

        // 在后台 spawn 执行任务
        tokio::spawn(async move {
            // 获取信号量许可（限制并发）
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    warn!(subagent_id = %id_clone, "Failed to acquire semaphore permit");
                    let _ = manager
                        .mark_failed(&id_clone, "Failed to acquire execution permit".to_string())
                        .await;
                    return;
                }
            };

            info!(subagent_id = %id_clone, "Starting Sub-Agent execution");

            // 获取 Sub-Agent 信息并创建上下文
            let context = {
                let agents = manager.agents.read().await;
                match agents.get(&id_clone) {
                    Some(agent) => SubAgentContext::from_agent(agent),
                    None => {
                        warn!(subagent_id = %id_clone, "Sub-Agent not found when starting execution");
                        return;
                    }
                }
            };

            // 创建执行器
            let executor = SubAgentExecutor::new(
                state,
                chat_server,
                headers,
                Arc::clone(&manager),
                available_tools,
                model,
            );

            // 执行 Sub-Agent
            let result = executor
                .execute(
                    &id_clone,
                    context,
                    timeout,
                    max_iterations,
                    &cancel_token,
                    emitter.as_ref(),
                    None,
                )
                .await;

            // 记录结果
            match &result {
                Ok(sub_result) => {
                    info!(
                        subagent_id = %id_clone,
                        iterations = sub_result.metrics.total_iterations,
                        "Sub-Agent completed successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        subagent_id = %id_clone,
                        error = %e,
                        "Sub-Agent execution failed"
                    );
                }
            }

            // Permit 会在这里自动释放
        });

        Ok(())
    }

    /// 同步执行 Sub-Agent（等待完成）
    ///
    /// 此方法会阻塞直到 Sub-Agent 执行完成。
    /// 主要用于 `wait_for_completion` 场景。
    ///
    /// # Arguments
    ///
    /// * `id` - Sub-Agent ID
    /// * `state` - 应用状态
    /// * `chat_server` - 聊天服务器信息
    /// * `headers` - HTTP 头信息
    /// * `available_tools` - 可用工具列表
    /// * `model` - 模型名称
    /// * `emitter` - 事件发射器
    /// * `timeout` - 超时时间（可选，使用默认值如果未指定）
    /// * `max_iterations` - 最大迭代次数（可选，使用默认值如果未指定）
    ///
    /// # Returns
    ///
    /// 返回执行结果
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_sync(
        self: &Arc<Self>,
        id: &SubAgentId,
        state: Arc<AppState>,
        chat_server: TargetServerInfo,
        headers: HeaderMap,
        available_tools: Vec<ToolDescription>,
        model: String,
        emitter: &dyn EventEmitter,
        timeout: Option<Duration>,
        max_iterations: Option<u32>,
    ) -> ServerResult<SubAgentResult> {
        // 获取信号量许可（限制并发）
        let _permit = self.concurrency_semaphore.acquire().await.map_err(|_| {
            ServerError::Operation("Failed to acquire execution permit".to_string())
        })?;

        // 获取取消令牌
        let cancel_token = self
            .get_cancel_token(id)
            .await
            .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;

        // 获取超时和迭代限制
        let timeout = timeout.unwrap_or(Duration::from_secs(self.config.default_timeout_secs));
        let max_iterations = max_iterations.unwrap_or(self.config.default_max_iterations);

        // 获取 Sub-Agent 信息并创建上下文
        let context = {
            let agents = self.agents.read().await;
            let agent = agents
                .get(id)
                .ok_or_else(|| ServerError::SubAgentNotFound(id.to_string()))?;
            SubAgentContext::from_agent(agent)
        };

        // 创建执行器
        let executor = SubAgentExecutor::new(
            state,
            chat_server,
            headers,
            Arc::clone(self),
            available_tools,
            model,
        );

        // 执行 Sub-Agent
        executor
            .execute(
                id,
                context,
                timeout,
                max_iterations,
                &cancel_token,
                emitter,
                None,
            )
            .await
    }

    /// 等待 Sub-Agent 完成
    ///
    /// 轮询检查 Sub-Agent 状态直到进入终态。
    ///
    /// # Arguments
    ///
    /// * `id` - Sub-Agent ID
    /// * `poll_interval` - 轮询间隔
    /// * `timeout` - 超时时间
    ///
    /// # Returns
    ///
    /// 返回 Sub-Agent 结果（如果有）
    pub async fn wait_for_completion(
        &self,
        id: &SubAgentId,
        poll_interval: Duration,
        timeout: Duration,
    ) -> ServerResult<Option<SubAgentResult>> {
        let start = std::time::Instant::now();

        loop {
            // 检查超时
            if start.elapsed() > timeout {
                return Err(ServerError::SubAgentTimeout {
                    id: id.to_string(),
                    timeout_secs: timeout.as_secs(),
                });
            }

            // 检查状态
            let state = self.get_state(id).await?;
            if state.is_terminal() {
                return self.get_result(id).await;
            }

            // 等待一段时间后再检查
            tokio::time::sleep(poll_interval).await;
        }
    }
}

// ============================================================================
// SubAgentStats
// ============================================================================

/// Sub-Agent 统计信息
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SubAgentStats {
    /// 总数
    pub total: usize,
    /// 等待中
    pub pending: usize,
    /// 运行中
    pub running: usize,
    /// 已完成
    pub completed: usize,
    /// 已失败
    pub failed: usize,
    /// 已取消
    pub cancelled: usize,
    /// 当前会话中所有 Sub-Agent 的提示词 token 总数
    pub total_prompt_tokens: u64,
    /// 当前会话中所有 Sub-Agent 的完成 token 总数
    pub total_completion_tokens: u64,
    /// 全局提示词 token 使用量（累计）
    pub global_prompt_tokens: u64,
    /// 全局完成 token 使用量（累计）
    pub global_completion_tokens: u64,
    /// 最大允许的全局 token 数（0 = 不限制）
    pub max_total_tokens: u64,
    /// 是否已超出 token 限制
    pub token_limit_exceeded: bool,
}

impl SubAgentStats {
    /// 获取当前会话 token 总数
    pub fn total_tokens(&self) -> u64 {
        self.total_prompt_tokens + self.total_completion_tokens
    }

    /// 获取全局 token 总数
    pub fn global_total_tokens(&self) -> u64 {
        self.global_prompt_tokens + self.global_completion_tokens
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn create_enabled_config() -> SubAgentSystemConfig {
        SubAgentSystemConfig::default_enabled()
    }

    fn create_limited_config(max_concurrent: usize, max_depth: u32) -> SubAgentSystemConfig {
        let mut config = SubAgentSystemConfig::default_enabled();
        config.max_concurrent = max_concurrent;
        config.max_nesting_depth = max_depth;
        config
    }

    #[tokio::test]
    async fn test_manager_creation() {
        let config = create_enabled_config();
        let manager = SubAgentManager::new(config);
        assert!(manager.is_enabled());
        assert_eq!(manager.running_count(), 0);
    }

    #[tokio::test]
    async fn test_spawn_basic() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn(
                "TestAgent",
                "You are a test agent.",
                "Do a test task",
                None,
                None,
            )
            .await
            .unwrap();

        assert!(id.as_str().starts_with("sa-"));

        let agent = manager.get(&id).await.unwrap();
        assert_eq!(agent.name, "TestAgent");
        assert_eq!(agent.state, SubAgentState::Pending);
        assert_eq!(agent.depth, 0);
    }

    #[tokio::test]
    async fn test_spawn_disabled() {
        let config = SubAgentSystemConfig::disabled();
        let manager = SubAgentManager::new(config);

        let result = manager
            .spawn("TestAgent", "system", "task", None, None)
            .await;

        assert!(matches!(result, Err(ServerError::SubAgentDisabled)));
    }

    #[tokio::test]
    async fn test_spawn_concurrent_limit() {
        let manager = SubAgentManager::new(create_limited_config(2, 3));

        // Spawn first two
        let id1 = manager
            .spawn("Agent1", "system", "task", None, None)
            .await
            .unwrap();
        let id2 = manager
            .spawn("Agent2", "system", "task", None, None)
            .await
            .unwrap();

        // Mark both as running
        manager.mark_started(&id1).await.unwrap();
        manager.mark_started(&id2).await.unwrap();

        // Third should fail
        let result = manager.spawn("Agent3", "system", "task", None, None).await;

        assert!(matches!(
            result,
            Err(ServerError::SubAgentLimitExceeded { max: 2 })
        ));
    }

    #[tokio::test]
    async fn test_spawn_nesting_depth_limit() {
        let manager = SubAgentManager::new(create_limited_config(10, 1));

        // Create parent (depth 0)
        let parent_id = manager
            .spawn("Parent", "system", "task", None, None)
            .await
            .unwrap();

        // Create child (depth 1)
        let child_id = manager
            .spawn("Child", "system", "task", None, Some(parent_id.clone()))
            .await
            .unwrap();

        let child = manager.get(&child_id).await.unwrap();
        assert_eq!(child.depth, 1);

        // Try to create grandchild (depth 2) - should fail with max_depth=1
        let result = manager
            .spawn("Grandchild", "system", "task", None, Some(child_id))
            .await;

        assert!(matches!(
            result,
            Err(ServerError::SubAgentMaxDepthExceeded {
                max_depth: 1,
                current_depth: 2
            })
        ));
    }

    #[tokio::test]
    async fn test_get_not_found() {
        let manager = SubAgentManager::new(create_enabled_config());
        let fake_id = SubAgentId::from_string("sa-nonexistent");

        let result = manager.get(&fake_id).await;
        assert!(matches!(result, Err(ServerError::SubAgentNotFound(_))));
    }

    #[tokio::test]
    async fn test_list() {
        let manager = SubAgentManager::new(create_enabled_config());

        manager
            .spawn("Agent1", "system", "task1", None, None)
            .await
            .unwrap();
        manager
            .spawn("Agent2", "system", "task2", None, None)
            .await
            .unwrap();

        let agents = manager.list().await;
        assert_eq!(agents.len(), 2);
    }

    #[tokio::test]
    async fn test_list_by_state() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id1 = manager
            .spawn("Agent1", "system", "task1", None, None)
            .await
            .unwrap();
        let _id2 = manager
            .spawn("Agent2", "system", "task2", None, None)
            .await
            .unwrap();

        // Mark one as running
        manager.mark_started(&id1).await.unwrap();

        let pending = manager.list_by_state(SubAgentState::Pending).await;
        let running = manager.list_by_state(SubAgentState::Running).await;

        assert_eq!(pending.len(), 1);
        assert_eq!(running.len(), 1);
    }

    #[tokio::test]
    async fn test_cancel() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();
        assert_eq!(manager.running_count(), 1);

        manager.cancel(&id).await.unwrap();

        let agent = manager.get(&id).await.unwrap();
        assert_eq!(agent.state, SubAgentState::Cancelled);
        assert_eq!(manager.running_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_already_terminal() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();
        manager.cancel(&id).await.unwrap();

        // Try to cancel again
        let result = manager.cancel(&id).await;
        assert!(matches!(
            result,
            Err(ServerError::SubAgentAlreadyTerminal { .. })
        ));
    }

    #[tokio::test]
    async fn test_mark_completed() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();
        assert_eq!(manager.running_count(), 1);

        let result = SubAgentResult::success("Done", Default::default());
        manager.mark_completed(&id, result).await.unwrap();

        let agent = manager.get(&id).await.unwrap();
        assert_eq!(agent.state, SubAgentState::Completed);
        assert!(agent.result.is_some());
        assert_eq!(manager.running_count(), 0);
    }

    #[tokio::test]
    async fn test_mark_failed() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();
        manager
            .mark_failed(&id, "Something went wrong".to_string())
            .await
            .unwrap();

        let agent = manager.get(&id).await.unwrap();
        assert_eq!(agent.state, SubAgentState::Failed);
        assert_eq!(manager.running_count(), 0);
    }

    #[tokio::test]
    async fn test_stats() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id1 = manager
            .spawn("Agent1", "system", "task1", None, None)
            .await
            .unwrap();
        let id2 = manager
            .spawn("Agent2", "system", "task2", None, None)
            .await
            .unwrap();
        let id3 = manager
            .spawn("Agent3", "system", "task3", None, None)
            .await
            .unwrap();

        manager.mark_started(&id1).await.unwrap();
        manager.mark_started(&id2).await.unwrap();
        manager
            .mark_completed(&id1, SubAgentResult::success("Done", Default::default()))
            .await
            .unwrap();
        manager.cancel(&id3).await.unwrap();

        let stats = manager.stats().await;
        assert_eq!(stats.total, 3);
        assert_eq!(stats.pending, 0);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.completed, 1);
        assert_eq!(stats.cancelled, 1);
    }

    #[tokio::test]
    async fn test_cleanup_completed() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id1 = manager
            .spawn("Agent1", "system", "task1", None, None)
            .await
            .unwrap();
        let id2 = manager
            .spawn("Agent2", "system", "task2", None, None)
            .await
            .unwrap();

        manager.mark_started(&id1).await.unwrap();
        manager
            .mark_completed(&id1, SubAgentResult::success("Done", Default::default()))
            .await
            .unwrap();

        let cleaned = manager.cleanup_completed().await;
        assert_eq!(cleaned, 1);

        // id1 should be gone, id2 should remain
        assert!(manager.get(&id1).await.is_err());
        assert!(manager.get(&id2).await.is_ok());
    }

    #[tokio::test]
    async fn test_list_children() {
        let manager = SubAgentManager::new(create_enabled_config());

        let parent_id = manager
            .spawn("Parent", "system", "task", None, None)
            .await
            .unwrap();

        let _child1 = manager
            .spawn("Child1", "system", "task", None, Some(parent_id.clone()))
            .await
            .unwrap();
        let _child2 = manager
            .spawn("Child2", "system", "task", None, Some(parent_id.clone()))
            .await
            .unwrap();
        let _other = manager
            .spawn("Other", "system", "task", None, None)
            .await
            .unwrap();

        let children = manager.list_children(&parent_id).await;
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_get_cancel_token() {
        let manager = SubAgentManager::new(create_enabled_config());

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        let token = manager.get_cancel_token(&id).await;
        assert!(token.is_some());
        assert!(!token.unwrap().is_cancelled());

        manager.cancel(&id).await.unwrap();

        let token = manager.get_cancel_token(&id).await;
        assert!(token.is_some());
        assert!(token.unwrap().is_cancelled());
    }

    // ========================================================================
    // Resource Control Tests
    // ========================================================================

    #[test]
    fn test_global_token_tracking() {
        let config = create_enabled_config();
        let manager = SubAgentManager::new(config);

        // Initial state
        assert_eq!(manager.global_total_tokens(), 0);
        let (prompt, completion) = manager.global_token_usage();
        assert_eq!(prompt, 0);
        assert_eq!(completion, 0);

        // Add tokens
        manager.add_global_tokens(100, 50);
        assert_eq!(manager.global_total_tokens(), 150);

        let (prompt, completion) = manager.global_token_usage();
        assert_eq!(prompt, 100);
        assert_eq!(completion, 50);

        // Add more tokens
        manager.add_global_tokens(200, 100);
        assert_eq!(manager.global_total_tokens(), 450);

        let (prompt, completion) = manager.global_token_usage();
        assert_eq!(prompt, 300);
        assert_eq!(completion, 150);
    }

    #[test]
    fn test_token_limit_not_exceeded_when_disabled() {
        let config = create_enabled_config(); // max_total_tokens = 0 (disabled)
        let manager = SubAgentManager::new(config);

        // Add many tokens
        manager.add_global_tokens(1_000_000, 1_000_000);

        // Should not be exceeded because limit is disabled
        assert!(!manager.is_token_limit_exceeded());
    }

    #[test]
    fn test_token_limit_exceeded() {
        let mut config = create_enabled_config();
        config.max_total_tokens = 1000; // Set limit
        let manager = SubAgentManager::new(config);

        // Add tokens below limit
        manager.add_global_tokens(400, 400);
        assert!(!manager.is_token_limit_exceeded());

        // Add more tokens to exceed limit
        manager.add_global_tokens(100, 200);
        assert!(manager.is_token_limit_exceeded());
    }

    #[test]
    fn test_reset_global_tokens() {
        let config = create_enabled_config();
        let manager = SubAgentManager::new(config);

        // Add tokens
        manager.add_global_tokens(500, 500);
        assert_eq!(manager.global_total_tokens(), 1000);

        // Reset
        manager.reset_global_tokens();
        assert_eq!(manager.global_total_tokens(), 0);
    }

    #[tokio::test]
    async fn test_stats_includes_global_tokens() {
        let mut config = create_enabled_config();
        config.max_total_tokens = 10000;
        let manager = SubAgentManager::new(config);

        // Add global tokens
        manager.add_global_tokens(1000, 500);

        let stats = manager.stats().await;
        assert_eq!(stats.global_prompt_tokens, 1000);
        assert_eq!(stats.global_completion_tokens, 500);
        assert_eq!(stats.global_total_tokens(), 1500);
        assert_eq!(stats.max_total_tokens, 10000);
        assert!(!stats.token_limit_exceeded);
    }

    #[tokio::test]
    async fn test_start_time_tracking() {
        let config = create_enabled_config();
        let manager = SubAgentManager::new(config);

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        // Before start, no start time
        {
            let start_times = manager.start_times.read().await;
            assert!(!start_times.contains_key(&id));
        }

        // After start, start time should be recorded
        manager.mark_started(&id).await.unwrap();
        {
            let start_times = manager.start_times.read().await;
            assert!(start_times.contains_key(&id));
        }

        // After completion, start time should be removed
        manager
            .mark_completed(&id, SubAgentResult::success("Done", Default::default()))
            .await
            .unwrap();
        {
            let start_times = manager.start_times.read().await;
            assert!(!start_times.contains_key(&id));
        }
    }

    #[tokio::test]
    async fn test_start_time_removed_on_failure() {
        let config = create_enabled_config();
        let manager = SubAgentManager::new(config);

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();
        {
            let start_times = manager.start_times.read().await;
            assert!(start_times.contains_key(&id));
        }

        manager.mark_failed(&id, "Error".to_string()).await.unwrap();
        {
            let start_times = manager.start_times.read().await;
            assert!(!start_times.contains_key(&id));
        }
    }

    #[tokio::test]
    async fn test_check_and_cancel_timed_out() {
        let mut config = create_enabled_config();
        config.default_timeout_secs = 0; // Immediate timeout for testing
        let manager = SubAgentManager::new(config);

        let id = manager
            .spawn("Agent", "system", "task", None, None)
            .await
            .unwrap();

        manager.mark_started(&id).await.unwrap();

        // Wait a tiny bit to ensure timeout
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Check and cancel timed out
        let cancelled = manager.check_and_cancel_timed_out().await;
        assert_eq!(cancelled, 1);

        // Verify agent is cancelled
        let agent = manager.get(&id).await.unwrap();
        assert_eq!(agent.state, SubAgentState::Cancelled);
    }
}
