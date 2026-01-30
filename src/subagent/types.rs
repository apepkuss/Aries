//! Sub-Agent 核心类型定义
//!
//! 此模块定义 Sub-Agent 系统的核心数据类型。

use std::{collections::HashSet, fmt, time::Duration};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// SubAgentId - Sub-Agent 唯一标识符
// ============================================================================

/// Sub-Agent 的唯一标识符
///
/// 使用 newtype pattern 封装字符串 ID，格式为 `sa-{uuid}`
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SubAgentId(String);

impl SubAgentId {
    /// 创建新的唯一 SubAgentId
    pub fn new() -> Self {
        Self(format!("sa-{}", Uuid::new_v4().simple()))
    }

    /// 从字符串创建 SubAgentId（用于反序列化或测试）
    pub fn from_string(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// 获取内部字符串引用
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SubAgentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for SubAgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl AsRef<str> for SubAgentId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

// ============================================================================
// SubAgentState - Sub-Agent 状态
// ============================================================================

/// Sub-Agent 的执行状态
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubAgentState {
    /// 等待执行
    #[default]
    Pending,
    /// 正在执行
    Running,
    /// 执行完成
    Completed,
    /// 执行失败
    Failed,
    /// 已取消
    Cancelled,
}

impl SubAgentState {
    /// 检查状态是否为终态（不会再改变）
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SubAgentState::Completed | SubAgentState::Failed | SubAgentState::Cancelled
        )
    }

    /// 检查状态是否为成功完成
    pub fn is_success(&self) -> bool {
        matches!(self, SubAgentState::Completed)
    }

    /// 检查是否可以转换到目标状态
    pub fn can_transition_to(&self, target: SubAgentState) -> bool {
        match (self, target) {
            // Pending 可以转换到 Running 或 Cancelled
            (SubAgentState::Pending, SubAgentState::Running) => true,
            (SubAgentState::Pending, SubAgentState::Cancelled) => true,
            // Running 可以转换到 Completed、Failed 或 Cancelled
            (SubAgentState::Running, SubAgentState::Completed) => true,
            (SubAgentState::Running, SubAgentState::Failed) => true,
            (SubAgentState::Running, SubAgentState::Cancelled) => true,
            // 终态不能再转换
            _ => false,
        }
    }
}

impl fmt::Display for SubAgentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubAgentState::Pending => write!(f, "pending"),
            SubAgentState::Running => write!(f, "running"),
            SubAgentState::Completed => write!(f, "completed"),
            SubAgentState::Failed => write!(f, "failed"),
            SubAgentState::Cancelled => write!(f, "cancelled"),
        }
    }
}

// ============================================================================
// SubAgentMetrics - Sub-Agent 执行指标
// ============================================================================

/// Sub-Agent 执行的资源使用指标
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubAgentMetrics {
    /// 总迭代次数
    pub total_iterations: u32,
    /// 工具调用次数
    pub tool_calls: u32,
    /// 提示词 token 数
    pub prompt_tokens: u64,
    /// 完成 token 数
    pub completion_tokens: u64,
    /// 执行时长（毫秒）
    pub duration_ms: u64,
    /// Time spent in tool execution / HITL wait (nanoseconds).
    /// Used to propagate pause duration from executor to plan-level time budget.
    #[serde(default)]
    pub tool_pause_nanos: u64,
}

impl SubAgentMetrics {
    /// 创建新的空指标
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取总 token 数
    pub fn total_tokens(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }

    /// 记录一次迭代
    pub fn record_iteration(&mut self) {
        self.total_iterations += 1;
    }

    /// 记录一次工具调用
    pub fn record_tool_call(&mut self) {
        self.tool_calls += 1;
    }

    /// 记录 token 使用
    pub fn record_tokens(&mut self, prompt: u64, completion: u64) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
    }

    /// 设置执行时长
    pub fn set_duration(&mut self, duration: Duration) {
        self.duration_ms = duration.as_millis() as u64;
    }
}

// ============================================================================
// SubAgentResult - Sub-Agent 执行结果
// ============================================================================

/// Sub-Agent 执行完成后的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentResult {
    /// 输出内容
    pub output: String,
    /// 生成的 artifact IDs（如果有）
    #[serde(default)]
    pub artifacts: Vec<String>,
    /// 执行指标
    pub metrics: SubAgentMetrics,
    /// 错误信息（如果失败）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl SubAgentResult {
    /// 创建成功结果
    pub fn success(output: impl Into<String>, metrics: SubAgentMetrics) -> Self {
        Self {
            output: output.into(),
            artifacts: Vec::new(),
            metrics,
            error: None,
        }
    }

    /// 创建失败结果
    pub fn failure(error: impl Into<String>, metrics: SubAgentMetrics) -> Self {
        Self {
            output: String::new(),
            artifacts: Vec::new(),
            metrics,
            error: Some(error.into()),
        }
    }

    /// 添加 artifact
    pub fn with_artifact(mut self, artifact_id: impl Into<String>) -> Self {
        self.artifacts.push(artifact_id.into());
        self
    }

    /// 添加多个 artifacts
    pub fn with_artifacts(mut self, artifact_ids: Vec<String>) -> Self {
        self.artifacts.extend(artifact_ids);
        self
    }

    /// 检查是否成功
    pub fn is_success(&self) -> bool {
        self.error.is_none()
    }
}

// ============================================================================
// SubAgent - Sub-Agent 实体
// ============================================================================

/// Sub-Agent 实体，表示一个可独立执行任务的子代理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgent {
    /// 唯一标识符
    pub id: SubAgentId,
    /// 显示名称
    pub name: String,
    /// 系统提示词
    pub system_prompt: String,
    /// 任务描述
    pub task: String,
    /// 当前状态
    pub state: SubAgentState,
    /// 子任务 ID（1-based，用于 HITL 显示标识）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtask_id: Option<usize>,
    /// 父 Sub-Agent ID（如果是嵌套创建的）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<SubAgentId>,
    /// 嵌套深度（0 表示由主 Agent 直接创建）
    pub depth: u32,
    /// 允许使用的工具列表（None 表示使用默认配置）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<HashSet<String>>,
    /// 禁止使用的工具列表
    #[serde(default)]
    pub blocked_tools: HashSet<String>,
    /// 创建时间戳（Unix 毫秒）
    pub created_at: u64,
    /// 开始执行时间戳
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<u64>,
    /// 完成时间戳
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<u64>,
    /// 执行结果
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<SubAgentResult>,
    /// 执行指标（实时更新）
    #[serde(default)]
    pub metrics: SubAgentMetrics,
}

impl SubAgent {
    /// 创建新的 Sub-Agent
    pub fn new(
        name: impl Into<String>,
        system_prompt: impl Into<String>,
        task: impl Into<String>,
    ) -> Self {
        Self {
            id: SubAgentId::new(),
            name: name.into(),
            system_prompt: system_prompt.into(),
            task: task.into(),
            state: SubAgentState::Pending,
            subtask_id: None,
            parent_id: None,
            depth: 0,
            allowed_tools: None,
            blocked_tools: HashSet::new(),
            created_at: current_timestamp_ms(),
            started_at: None,
            completed_at: None,
            result: None,
            metrics: SubAgentMetrics::new(),
        }
    }

    /// 设置父 Sub-Agent
    pub fn with_parent(mut self, parent_id: SubAgentId, parent_depth: u32) -> Self {
        self.parent_id = Some(parent_id);
        self.depth = parent_depth + 1;
        self
    }

    /// 设置子任务 ID（1-based，用于 HITL 显示标识）
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }

    /// 设置允许的工具
    pub fn with_allowed_tools(mut self, tools: HashSet<String>) -> Self {
        self.allowed_tools = Some(tools);
        self
    }

    /// 设置禁止的工具
    pub fn with_blocked_tools(mut self, tools: HashSet<String>) -> Self {
        self.blocked_tools = tools;
        self
    }

    /// 标记为开始执行
    pub fn start(&mut self) -> bool {
        if self.state.can_transition_to(SubAgentState::Running) {
            self.state = SubAgentState::Running;
            self.started_at = Some(current_timestamp_ms());
            true
        } else {
            false
        }
    }

    /// 标记为完成
    pub fn complete(&mut self, result: SubAgentResult) -> bool {
        if self.state.can_transition_to(SubAgentState::Completed) {
            self.state = SubAgentState::Completed;
            self.completed_at = Some(current_timestamp_ms());
            self.metrics = result.metrics.clone();
            self.result = Some(result);
            true
        } else {
            false
        }
    }

    /// 标记为失败
    pub fn fail(&mut self, error: impl Into<String>) -> bool {
        if self.state.can_transition_to(SubAgentState::Failed) {
            self.state = SubAgentState::Failed;
            self.completed_at = Some(current_timestamp_ms());
            self.result = Some(SubAgentResult::failure(error, self.metrics.clone()));
            true
        } else {
            false
        }
    }

    /// 标记为取消
    pub fn cancel(&mut self) -> bool {
        if self.state.can_transition_to(SubAgentState::Cancelled) {
            self.state = SubAgentState::Cancelled;
            self.completed_at = Some(current_timestamp_ms());
            true
        } else {
            false
        }
    }

    /// 检查工具是否被允许
    pub fn is_tool_allowed(&self, tool_name: &str) -> bool {
        // 先检查是否在禁止列表中
        if self.blocked_tools.contains(tool_name) {
            return false;
        }
        // 如果有允许列表，检查是否在其中
        if let Some(allowed) = &self.allowed_tools {
            return allowed.contains(tool_name);
        }
        // 默认允许
        true
    }

    /// 获取执行时长
    pub fn duration(&self) -> Option<Duration> {
        match (self.started_at, self.completed_at) {
            (Some(start), Some(end)) => Some(Duration::from_millis(end - start)),
            (Some(start), None) => {
                let now = current_timestamp_ms();
                Some(Duration::from_millis(now - start))
            }
            _ => None,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 获取当前时间戳（Unix 毫秒）
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subagent_id_uniqueness() {
        let id1 = SubAgentId::new();
        let id2 = SubAgentId::new();
        assert_ne!(id1, id2);
        assert!(id1.as_str().starts_with("sa-"));
        assert!(id2.as_str().starts_with("sa-"));
    }

    #[test]
    fn test_subagent_id_display() {
        let id = SubAgentId::from_string("sa-test123");
        assert_eq!(format!("{}", id), "sa-test123");
    }

    #[test]
    fn test_subagent_id_serialization() {
        let id = SubAgentId::from_string("sa-test123");
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, "\"sa-test123\"");

        let deserialized: SubAgentId = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, id);
    }

    #[test]
    fn test_subagent_state_transitions() {
        // Pending -> Running
        assert!(SubAgentState::Pending.can_transition_to(SubAgentState::Running));
        // Pending -> Cancelled
        assert!(SubAgentState::Pending.can_transition_to(SubAgentState::Cancelled));
        // Pending -> Completed (invalid)
        assert!(!SubAgentState::Pending.can_transition_to(SubAgentState::Completed));

        // Running -> Completed
        assert!(SubAgentState::Running.can_transition_to(SubAgentState::Completed));
        // Running -> Failed
        assert!(SubAgentState::Running.can_transition_to(SubAgentState::Failed));
        // Running -> Cancelled
        assert!(SubAgentState::Running.can_transition_to(SubAgentState::Cancelled));

        // Terminal states cannot transition
        assert!(!SubAgentState::Completed.can_transition_to(SubAgentState::Running));
        assert!(!SubAgentState::Failed.can_transition_to(SubAgentState::Running));
        assert!(!SubAgentState::Cancelled.can_transition_to(SubAgentState::Running));
    }

    #[test]
    fn test_subagent_state_terminal() {
        assert!(!SubAgentState::Pending.is_terminal());
        assert!(!SubAgentState::Running.is_terminal());
        assert!(SubAgentState::Completed.is_terminal());
        assert!(SubAgentState::Failed.is_terminal());
        assert!(SubAgentState::Cancelled.is_terminal());
    }

    #[test]
    fn test_subagent_state_serialization() {
        let state = SubAgentState::Running;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"running\"");

        let deserialized: SubAgentState = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(deserialized, SubAgentState::Completed);
    }

    #[test]
    fn test_subagent_metrics() {
        let mut metrics = SubAgentMetrics::new();
        assert_eq!(metrics.total_iterations, 0);
        assert_eq!(metrics.total_tokens(), 0);

        metrics.record_iteration();
        metrics.record_tool_call();
        metrics.record_tokens(100, 50);

        assert_eq!(metrics.total_iterations, 1);
        assert_eq!(metrics.tool_calls, 1);
        assert_eq!(metrics.total_tokens(), 150);
    }

    #[test]
    fn test_subagent_result_success() {
        let metrics = SubAgentMetrics::new();
        let result = SubAgentResult::success("Task completed", metrics);
        assert!(result.is_success());
        assert_eq!(result.output, "Task completed");
        assert!(result.error.is_none());
    }

    #[test]
    fn test_subagent_result_failure() {
        let metrics = SubAgentMetrics::new();
        let result = SubAgentResult::failure("Something went wrong", metrics);
        assert!(!result.is_success());
        assert_eq!(result.error, Some("Something went wrong".to_string()));
    }

    #[test]
    fn test_subagent_creation() {
        let agent = SubAgent::new("TestAgent", "You are a test agent.", "Do something");
        assert!(agent.id.as_str().starts_with("sa-"));
        assert_eq!(agent.name, "TestAgent");
        assert_eq!(agent.state, SubAgentState::Pending);
        assert_eq!(agent.depth, 0);
        assert!(agent.parent_id.is_none());
    }

    #[test]
    fn test_subagent_with_parent() {
        let parent_id = SubAgentId::from_string("sa-parent");
        let agent = SubAgent::new("ChildAgent", "system", "task").with_parent(parent_id.clone(), 0);

        assert_eq!(agent.depth, 1);
        assert_eq!(agent.parent_id, Some(parent_id));
    }

    #[test]
    fn test_subagent_state_machine() {
        let mut agent = SubAgent::new("Agent", "system", "task");
        assert_eq!(agent.state, SubAgentState::Pending);

        // Start
        assert!(agent.start());
        assert_eq!(agent.state, SubAgentState::Running);
        assert!(agent.started_at.is_some());

        // Cannot start again
        assert!(!agent.start());

        // Complete
        let result = SubAgentResult::success("Done", SubAgentMetrics::new());
        assert!(agent.complete(result));
        assert_eq!(agent.state, SubAgentState::Completed);
        assert!(agent.completed_at.is_some());

        // Cannot change from terminal state
        assert!(!agent.fail("error"));
        assert!(!agent.cancel());
    }

    #[test]
    fn test_subagent_tool_access() {
        let mut allowed = HashSet::new();
        allowed.insert("tool_a".to_string());
        allowed.insert("tool_b".to_string());

        let mut blocked = HashSet::new();
        blocked.insert("tool_b".to_string()); // Both allowed and blocked

        let agent = SubAgent::new("Agent", "system", "task")
            .with_allowed_tools(allowed)
            .with_blocked_tools(blocked);

        // tool_a is allowed
        assert!(agent.is_tool_allowed("tool_a"));
        // tool_b is blocked (blocked takes precedence)
        assert!(!agent.is_tool_allowed("tool_b"));
        // tool_c is not in allowed list
        assert!(!agent.is_tool_allowed("tool_c"));
    }

    #[test]
    fn test_subagent_serialization() {
        let agent = SubAgent::new("TestAgent", "You are helpful.", "Complete the task");
        let json = serde_json::to_string(&agent).unwrap();

        assert!(json.contains("\"name\":\"TestAgent\""));
        assert!(json.contains("\"state\":\"pending\""));

        let deserialized: SubAgent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, agent.name);
        assert_eq!(deserialized.state, agent.state);
    }
}
