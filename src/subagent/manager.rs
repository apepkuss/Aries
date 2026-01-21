//! Sub-Agent 管理器
//!
//! 此模块实现 SubAgentManager，负责 Sub-Agent 的生命周期管理。

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use http::HeaderMap;
use tokio::sync::{RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

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
/// use aries::subagent::{SubAgentManager, SubAgentSystemConfig};
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

        stats
    }

    /// 清理已完成的 Sub-Agent（释放内存）
    ///
    /// 返回清理的数量
    pub async fn cleanup_completed(&self) -> usize {
        let mut agents = self.agents.write().await;
        let mut tokens = self.cancel_tokens.write().await;

        let to_remove: Vec<SubAgentId> = agents
            .iter()
            .filter(|(_, a)| a.state.is_terminal())
            .map(|(id, _)| id.clone())
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            agents.remove(&id);
            tokens.remove(&id);
        }

        count
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
            .execute(id, context, timeout, max_iterations, &cancel_token, emitter)
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
    /// 总提示词 token 数
    pub total_prompt_tokens: u64,
    /// 总完成 token 数
    pub total_completion_tokens: u64,
}

impl SubAgentStats {
    /// 获取总 token 数
    pub fn total_tokens(&self) -> u64 {
        self.total_prompt_tokens + self.total_completion_tokens
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
}
