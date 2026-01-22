//! Sub-Agent 配置类型定义
//!
//! 此模块定义 Sub-Agent 系统的配置结构。

use std::{collections::HashSet, time::Duration};

use serde::{Deserialize, Serialize};

// ============================================================================
// SubAgentSystemConfig - 系统级配置
// ============================================================================

/// Sub-Agent 系统级配置
///
/// 控制整个 Sub-Agent 子系统的行为
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentSystemConfig {
    /// 是否启用 Sub-Agent 功能
    #[serde(default)]
    pub enabled: bool,

    /// 执行模式: "direct" | "subagent"
    /// - direct: 直接执行（传统模式）
    /// - subagent: 作为 Subtask 执行器
    #[serde(default = "default_execution_mode")]
    pub execution_mode: String,

    /// 最大嵌套深度（0 = 不允许嵌套，1 = 主 Agent -> Sub-Agent，以此类推）
    #[serde(default = "default_max_nesting_depth")]
    pub max_nesting_depth: u32,

    /// 最大同时运行的 Sub-Agent 数量
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent: usize,

    /// 默认超时时间（秒）
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,

    /// 默认最大迭代次数
    #[serde(default = "default_max_iterations")]
    pub default_max_iterations: u32,

    /// 是否允许 Sub-Agent 访问所有工具（否则需要显式指定）
    #[serde(default)]
    pub allow_full_tool_access: bool,

    /// 默认允许的工具列表
    #[serde(default)]
    pub default_allowed_tools: HashSet<String>,

    /// 默认禁止的工具列表
    #[serde(default = "default_blocked_tools")]
    pub default_blocked_tools: HashSet<String>,

    /// 失败策略
    #[serde(default)]
    pub failure_policy: FailurePolicy,

    /// 重试次数（当 failure_policy 为 Retry 时）
    #[serde(default = "default_retry_attempts")]
    pub retry_attempts: u32,

    /// 所有 Sub-Agent 总计最大 token 数（0 = 不限制）
    #[serde(default)]
    pub max_total_tokens: u64,

    /// 反思（Reflection）配置
    #[serde(default)]
    pub reflection: SubAgentReflectionConfig,

    /// Subtask 执行器配置（当 execution_mode = "subagent" 时生效）
    #[serde(default)]
    pub subtask_executor: SubtaskExecutorConfig,

    /// 上下文传递配置
    #[serde(default)]
    pub context: SubAgentContextConfig,

    /// API 限流配置
    #[serde(default)]
    pub rate_limit: SubAgentRateLimitConfig,
}

impl SubAgentSystemConfig {
    /// 创建启用的默认配置
    pub fn default_enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// 创建禁用的配置
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// 获取默认超时时长
    pub fn default_timeout(&self) -> Duration {
        Duration::from_secs(self.default_timeout_secs)
    }

    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        // 验证 execution_mode
        let valid_modes = ["direct", "subagent"];
        if !valid_modes.contains(&self.execution_mode.as_str()) {
            return Err(ConfigValidationError::InvalidValue {
                field: "execution_mode".to_string(),
                message: format!(
                    "Invalid execution_mode '{}', must be one of: {:?}",
                    self.execution_mode, valid_modes
                ),
            });
        }

        if self.max_nesting_depth > 10 {
            return Err(ConfigValidationError::InvalidValue {
                field: "max_nesting_depth".to_string(),
                message: "Maximum nesting depth cannot exceed 10".to_string(),
            });
        }

        if self.max_concurrent == 0 && self.enabled {
            return Err(ConfigValidationError::InvalidValue {
                field: "max_concurrent".to_string(),
                message: "max_concurrent must be > 0 when enabled".to_string(),
            });
        }

        if self.default_timeout_secs == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "default_timeout_secs".to_string(),
                message: "default_timeout_secs must be > 0".to_string(),
            });
        }

        if self.default_max_iterations == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "default_max_iterations".to_string(),
                message: "default_max_iterations must be > 0".to_string(),
            });
        }

        // 验证嵌套配置
        if self.is_subagent_mode() {
            self.subtask_executor.validate()?;
            self.context.validate()?;
            self.rate_limit.validate()?;
        }

        Ok(())
    }

    /// 是否使用 subagent 执行模式
    pub fn is_subagent_mode(&self) -> bool {
        self.execution_mode == "subagent"
    }

    /// 是否使用 direct 执行模式
    pub fn is_direct_mode(&self) -> bool {
        self.execution_mode == "direct"
    }
}

impl Default for SubAgentSystemConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            execution_mode: default_execution_mode(),
            max_nesting_depth: default_max_nesting_depth(),
            max_concurrent: default_max_concurrent(),
            default_timeout_secs: default_timeout_secs(),
            default_max_iterations: default_max_iterations(),
            allow_full_tool_access: false,
            default_allowed_tools: HashSet::new(),
            default_blocked_tools: default_blocked_tools(),
            failure_policy: FailurePolicy::default(),
            retry_attempts: default_retry_attempts(),
            max_total_tokens: 0,
            reflection: SubAgentReflectionConfig::default(),
            subtask_executor: SubtaskExecutorConfig::default(),
            context: SubAgentContextConfig::default(),
            rate_limit: SubAgentRateLimitConfig::default(),
        }
    }
}

// ============================================================================
// SubtaskExecutorConfig - Subtask 执行器配置
// ============================================================================

/// Subtask 执行器配置（当 execution_mode = "subagent" 时生效）
///
/// 控制 Sub-Agent 作为 Subtask 执行者时的行为
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtaskExecutorConfig {
    /// 并行模式: "auto" | "sequential" | "manual"
    /// - auto: 根据依赖关系自动并行
    /// - sequential: 顺序执行
    /// - manual: LLM 手动标记并行任务
    #[serde(default = "default_parallel_mode")]
    pub parallel_mode: String,

    /// 最大并行执行数
    #[serde(default = "default_max_parallel")]
    pub max_parallel: usize,

    /// 是否继承所有父级工具
    #[serde(default = "default_true")]
    pub inherit_tools: bool,

    /// 是否继承技能上下文
    #[serde(default = "default_true")]
    pub inherit_skills: bool,

    /// 工具黑名单（在继承基础上排除的工具）
    #[serde(default = "default_subtask_blocked_tools")]
    pub blocked_tools: Vec<String>,

    /// 是否允许嵌套 spawn Sub-Agent
    #[serde(default)]
    pub allow_nested_spawn: bool,

    /// 单个 Subtask 超时（秒）
    #[serde(default = "default_subtask_timeout_secs")]
    pub timeout_secs: u64,

    /// 是否继承 Plan 剩余时间（取较小值）
    #[serde(default = "default_true")]
    pub inherit_remaining_time: bool,

    /// 优雅退出宽限期（秒）
    #[serde(default = "default_grace_period_secs")]
    pub grace_period_secs: u64,

    /// 失败策略
    #[serde(default)]
    pub failure_policy: FailurePolicy,

    /// 重试模式: "internal" | "respawn"
    /// - internal: Sub-Agent 内部重试
    /// - respawn: 销毁并重新创建 Sub-Agent
    #[serde(default = "default_retry_mode")]
    pub retry_mode: String,

    /// 最大重试次数
    #[serde(default = "default_subtask_max_retries")]
    pub max_retries: u32,

    /// 重试延迟（毫秒）
    #[serde(default = "default_retry_delay_ms")]
    pub retry_delay_ms: u64,

    /// 重试时是否注入失败上下文
    #[serde(default = "default_true")]
    pub inject_failure_context: bool,
}

impl Default for SubtaskExecutorConfig {
    fn default() -> Self {
        Self {
            parallel_mode: default_parallel_mode(),
            max_parallel: default_max_parallel(),
            inherit_tools: true,
            inherit_skills: true,
            blocked_tools: default_subtask_blocked_tools(),
            allow_nested_spawn: false,
            timeout_secs: default_subtask_timeout_secs(),
            inherit_remaining_time: true,
            grace_period_secs: default_grace_period_secs(),
            failure_policy: FailurePolicy::default(),
            retry_mode: default_retry_mode(),
            max_retries: default_subtask_max_retries(),
            retry_delay_ms: default_retry_delay_ms(),
            inject_failure_context: true,
        }
    }
}

impl SubtaskExecutorConfig {
    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        let valid_parallel_modes = ["auto", "sequential", "manual"];
        if !valid_parallel_modes.contains(&self.parallel_mode.as_str()) {
            return Err(ConfigValidationError::InvalidValue {
                field: "parallel_mode".to_string(),
                message: format!(
                    "Invalid parallel_mode '{}', must be one of: {:?}",
                    self.parallel_mode, valid_parallel_modes
                ),
            });
        }

        if self.max_parallel == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "max_parallel".to_string(),
                message: "max_parallel must be > 0".to_string(),
            });
        }

        let valid_retry_modes = ["internal", "respawn"];
        if !valid_retry_modes.contains(&self.retry_mode.as_str()) {
            return Err(ConfigValidationError::InvalidValue {
                field: "retry_mode".to_string(),
                message: format!(
                    "Invalid retry_mode '{}', must be one of: {:?}",
                    self.retry_mode, valid_retry_modes
                ),
            });
        }

        if self.timeout_secs == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "timeout_secs".to_string(),
                message: "timeout_secs must be > 0".to_string(),
            });
        }

        Ok(())
    }

    /// 是否使用 respawn 重试模式
    pub fn is_respawn_retry(&self) -> bool {
        self.retry_mode == "respawn"
    }

    /// 是否自动并行
    pub fn is_auto_parallel(&self) -> bool {
        self.parallel_mode == "auto"
    }
}

fn default_parallel_mode() -> String {
    "auto".to_string()
}

fn default_max_parallel() -> usize {
    3
}

fn default_subtask_blocked_tools() -> Vec<String> {
    vec![
        "internal__file_write".to_string(),
        "internal__file_delete".to_string(),
        "internal__shell_exec".to_string(),
    ]
}

fn default_subtask_timeout_secs() -> u64 {
    120 // 2 minutes
}

fn default_grace_period_secs() -> u64 {
    5
}

fn default_retry_mode() -> String {
    "respawn".to_string()
}

fn default_subtask_max_retries() -> u32 {
    2
}

fn default_retry_delay_ms() -> u64 {
    1000
}

// ============================================================================
// SubAgentContextConfig - 上下文传递配置
// ============================================================================

/// Sub-Agent 上下文传递配置
///
/// 控制前序 Subtask 结果如何传递给后续 Sub-Agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentContextConfig {
    /// 上下文传递模式: "inject" | "shared" | "hybrid"
    /// - inject: 将前序结果注入到 System Prompt
    /// - shared: 使用共享存储，按需查询
    /// - hybrid: 结合两种方式
    #[serde(default = "default_context_mode")]
    pub mode: String,

    /// 注入上下文的最大 token 数
    #[serde(default = "default_max_inject_tokens")]
    pub max_inject_tokens: usize,

    /// 超过上限时是否使用摘要
    #[serde(default = "default_true")]
    pub use_summary: bool,

    /// 是否启用 Artifact 共享存储
    #[serde(default)]
    pub artifact_storage: bool,
}

impl Default for SubAgentContextConfig {
    fn default() -> Self {
        Self {
            mode: default_context_mode(),
            max_inject_tokens: default_max_inject_tokens(),
            use_summary: true,
            artifact_storage: false,
        }
    }
}

impl SubAgentContextConfig {
    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        let valid_modes = ["inject", "shared", "hybrid"];
        if !valid_modes.contains(&self.mode.as_str()) {
            return Err(ConfigValidationError::InvalidValue {
                field: "mode".to_string(),
                message: format!(
                    "Invalid context mode '{}', must be one of: {:?}",
                    self.mode, valid_modes
                ),
            });
        }

        if self.max_inject_tokens == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "max_inject_tokens".to_string(),
                message: "max_inject_tokens must be > 0".to_string(),
            });
        }

        Ok(())
    }

    /// 是否使用注入模式
    pub fn is_inject_mode(&self) -> bool {
        self.mode == "inject" || self.mode == "hybrid"
    }

    /// 是否使用共享存储模式
    pub fn is_shared_mode(&self) -> bool {
        self.mode == "shared" || self.mode == "hybrid"
    }
}

fn default_context_mode() -> String {
    "inject".to_string()
}

fn default_max_inject_tokens() -> usize {
    2000
}

// ============================================================================
// SubAgentRateLimitConfig - API 限流配置
// ============================================================================

/// Sub-Agent API 限流配置
///
/// 使用令牌桶算法控制 LLM API 调用频率
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentRateLimitConfig {
    /// 每分钟最大请求数 (RPM)
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,

    /// 每分钟最大 token 数 (TPM)
    #[serde(default = "default_tokens_per_minute")]
    pub tokens_per_minute: u64,

    /// 触发限流后的最大重试次数
    #[serde(default = "default_rate_limit_retry_attempts")]
    pub retry_max_attempts: u32,

    /// 重试基础延迟（毫秒），使用指数退避
    #[serde(default = "default_rate_limit_retry_delay")]
    pub retry_base_delay_ms: u64,
}

impl Default for SubAgentRateLimitConfig {
    fn default() -> Self {
        Self {
            requests_per_minute: default_requests_per_minute(),
            tokens_per_minute: default_tokens_per_minute(),
            retry_max_attempts: default_rate_limit_retry_attempts(),
            retry_base_delay_ms: default_rate_limit_retry_delay(),
        }
    }
}

impl SubAgentRateLimitConfig {
    /// 验证配置有效性
    pub fn validate(&self) -> Result<(), ConfigValidationError> {
        if self.requests_per_minute == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "requests_per_minute".to_string(),
                message: "requests_per_minute must be > 0".to_string(),
            });
        }

        if self.tokens_per_minute == 0 {
            return Err(ConfigValidationError::InvalidValue {
                field: "tokens_per_minute".to_string(),
                message: "tokens_per_minute must be > 0".to_string(),
            });
        }

        Ok(())
    }

    /// 计算指数退避延迟
    pub fn retry_delay_for_attempt(&self, attempt: u32) -> std::time::Duration {
        let delay_ms = self.retry_base_delay_ms * (1 << attempt.min(5));
        std::time::Duration::from_millis(delay_ms)
    }
}

fn default_requests_per_minute() -> u32 {
    60
}

fn default_tokens_per_minute() -> u64 {
    100000
}

fn default_rate_limit_retry_attempts() -> u32 {
    3
}

fn default_rate_limit_retry_delay() -> u64 {
    1000
}

// ============================================================================
// SubAgentReflectionConfig - 反思配置
// ============================================================================

/// Sub-Agent 反思配置
///
/// 控制 Sub-Agent 执行过程中的自我评估和纠错机制
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubAgentReflectionConfig {
    /// 是否启用反思
    #[serde(default)]
    pub enabled: bool,

    /// 触发反思的迭代间隔（每 N 次迭代触发一次反思）
    /// 0 表示仅在完成时反思
    #[serde(default = "default_reflection_interval")]
    pub interval_iterations: u32,

    /// 触发反思的最小迭代次数
    /// 低于此迭代数不触发反思（避免简单任务浪费 Token）
    #[serde(default = "default_min_iterations_for_reflection")]
    pub min_iterations: u32,

    /// 是否在工具调用错误后触发反思
    #[serde(default = "default_true")]
    pub reflect_on_tool_error: bool,

    /// 是否在完成时进行最终反思
    #[serde(default = "default_true")]
    pub reflect_on_completion: bool,

    /// 置信度阈值（低于此值触发深度反思）
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: f64,

    /// 最大反思重试次数
    #[serde(default = "default_max_reflection_retries")]
    pub max_retries: u32,
}

impl Default for SubAgentReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_iterations: default_reflection_interval(),
            min_iterations: default_min_iterations_for_reflection(),
            reflect_on_tool_error: true,
            reflect_on_completion: true,
            confidence_threshold: default_confidence_threshold(),
            max_retries: default_max_reflection_retries(),
        }
    }
}

impl SubAgentReflectionConfig {
    /// 创建启用反思的配置
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            ..Default::default()
        }
    }

    /// 创建禁用反思的配置
    pub fn disabled() -> Self {
        Self::default()
    }

    /// 设置迭代间隔
    pub fn with_interval(mut self, iterations: u32) -> Self {
        self.interval_iterations = iterations;
        self
    }

    /// 设置最小迭代次数
    pub fn with_min_iterations(mut self, min: u32) -> Self {
        self.min_iterations = min;
        self
    }

    /// 判断是否应在指定迭代触发反思
    pub fn should_reflect_at_iteration(&self, iteration: u32) -> bool {
        if !self.enabled {
            return false;
        }

        // 未达到最小迭代次数
        if iteration < self.min_iterations {
            return false;
        }

        // 间隔为 0 表示仅在完成时反思
        if self.interval_iterations == 0 {
            return false;
        }

        // 检查是否是反思间隔
        iteration.is_multiple_of(self.interval_iterations)
    }

    /// 判断是否应在完成时反思
    pub fn should_reflect_on_completion(&self, total_iterations: u32) -> bool {
        if !self.enabled {
            return false;
        }

        // 未达到最小迭代次数
        if total_iterations < self.min_iterations {
            return false;
        }

        self.reflect_on_completion
    }
}

fn default_reflection_interval() -> u32 {
    5 // 每 5 次迭代触发一次反思
}

fn default_min_iterations_for_reflection() -> u32 {
    3 // 至少 3 次迭代才触发反思
}

fn default_confidence_threshold() -> f64 {
    0.7
}

fn default_max_reflection_retries() -> u32 {
    2
}

// ============================================================================
// SubAgentSpawnConfig - 单个 Sub-Agent 的创建配置
// ============================================================================

/// 创建单个 Sub-Agent 时的配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubAgentSpawnConfig {
    /// 超时时间（秒），None 表示使用系统默认
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u64>,

    /// 最大迭代次数，None 表示使用系统默认
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_iterations: Option<u32>,

    /// 工具访问配置
    #[serde(default)]
    pub tool_access: SubAgentToolAccess,

    /// 是否等待完成（同步执行）
    #[serde(default)]
    pub wait_for_completion: bool,
}

impl SubAgentSpawnConfig {
    /// 创建新的配置
    pub fn new() -> Self {
        Self::default()
    }

    /// 设置超时
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// 设置最大迭代次数
    pub fn with_max_iterations(mut self, iterations: u32) -> Self {
        self.max_iterations = Some(iterations);
        self
    }

    /// 设置同步执行
    pub fn synchronous(mut self) -> Self {
        self.wait_for_completion = true;
        self
    }

    /// 设置允许的工具
    pub fn with_allowed_tools(mut self, tools: Vec<String>) -> Self {
        self.tool_access.allowed_tools = Some(tools.into_iter().collect());
        self
    }

    /// 设置禁止的工具
    pub fn with_blocked_tools(mut self, tools: Vec<String>) -> Self {
        self.tool_access.blocked_tools = tools.into_iter().collect();
        self
    }
}

// ============================================================================
// SubAgentToolAccess - 工具访问控制
// ============================================================================

/// Sub-Agent 工具访问控制配置
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SubAgentToolAccess {
    /// 允许的工具列表（None 表示使用系统默认或全部）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<HashSet<String>>,

    /// 禁止的工具列表
    #[serde(default)]
    pub blocked_tools: HashSet<String>,

    /// 是否继承父 Agent 的工具访问权限
    #[serde(default = "default_true")]
    pub inherit_from_parent: bool,
}

impl SubAgentToolAccess {
    /// 创建允许所有工具的配置
    pub fn allow_all() -> Self {
        Self {
            allowed_tools: None,
            blocked_tools: HashSet::new(),
            inherit_from_parent: false,
        }
    }

    /// 创建只允许特定工具的配置
    pub fn only(tools: Vec<String>) -> Self {
        Self {
            allowed_tools: Some(tools.into_iter().collect()),
            blocked_tools: HashSet::new(),
            inherit_from_parent: false,
        }
    }

    /// 创建禁止特定工具的配置
    pub fn except(tools: Vec<String>) -> Self {
        Self {
            allowed_tools: None,
            blocked_tools: tools.into_iter().collect(),
            inherit_from_parent: true,
        }
    }
}

// ============================================================================
// FailurePolicy - 失败处理策略
// ============================================================================

/// Sub-Agent 执行失败时的处理策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailurePolicy {
    /// 立即失败，将错误传播给主 Agent（与 FailFast 行为相同，保持兼容）
    #[default]
    Fail,
    /// 立即失败，终止整个执行流程
    FailFast,
    /// 通知主 Agent，让其决定如何处理
    Notify,
    /// 自动重试
    Retry,
    /// 忽略失败，继续执行（与 Skip 行为相同，保持兼容）
    Ignore,
    /// 跳过当前任务，继续执行后续任务
    Skip,
    /// 先重试，达到最大次数后跳过
    RetryThenSkip,
    /// 先重试，达到最大次数后失败
    RetryThenFail,
}

impl FailurePolicy {
    /// 是否需要重试
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            FailurePolicy::Retry | FailurePolicy::RetryThenSkip | FailurePolicy::RetryThenFail
        )
    }

    /// 是否需要通知主 Agent
    pub fn should_notify(&self) -> bool {
        matches!(
            self,
            FailurePolicy::Notify | FailurePolicy::Fail | FailurePolicy::FailFast
        )
    }

    /// 重试失败后是否跳过
    pub fn should_skip_after_retry(&self) -> bool {
        matches!(self, FailurePolicy::RetryThenSkip | FailurePolicy::Skip)
    }

    /// 重试失败后是否终止
    pub fn should_fail_after_retry(&self) -> bool {
        matches!(
            self,
            FailurePolicy::RetryThenFail | FailurePolicy::Fail | FailurePolicy::FailFast
        )
    }

    /// 是否立即跳过（不重试）
    pub fn should_skip_immediately(&self) -> bool {
        matches!(self, FailurePolicy::Skip | FailurePolicy::Ignore)
    }

    /// 是否立即失败（不重试）
    pub fn should_fail_immediately(&self) -> bool {
        matches!(self, FailurePolicy::Fail | FailurePolicy::FailFast)
    }
}

// ============================================================================
// ConfigValidationError - 配置验证错误
// ============================================================================

/// 配置验证错误
#[derive(Debug, Clone)]
pub enum ConfigValidationError {
    /// 无效的配置值
    InvalidValue { field: String, message: String },
}

impl std::fmt::Display for ConfigValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigValidationError::InvalidValue { field, message } => {
                write!(f, "Invalid config value for '{}': {}", field, message)
            }
        }
    }
}

impl std::error::Error for ConfigValidationError {}

// ============================================================================
// Default Value Functions
// ============================================================================

fn default_execution_mode() -> String {
    "direct".to_string()
}

fn default_max_nesting_depth() -> u32 {
    3
}

fn default_max_concurrent() -> usize {
    5
}

fn default_timeout_secs() -> u64 {
    300 // 5 minutes
}

fn default_max_iterations() -> u32 {
    20
}

fn default_retry_attempts() -> u32 {
    2
}

fn default_blocked_tools() -> HashSet<String> {
    let mut set = HashSet::new();
    // 默认禁止危险操作
    set.insert("mcp__filesystem__delete_file".to_string());
    set.insert("internal__spawn_sub_agent".to_string()); // 防止默认递归
    set
}

fn default_true() -> bool {
    true
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = SubAgentSystemConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.max_nesting_depth, 3);
        assert_eq!(config.max_concurrent, 5);
        assert_eq!(config.default_timeout_secs, 300);
        assert_eq!(config.default_max_iterations, 20);
    }

    #[test]
    fn test_enabled_config() {
        let config = SubAgentSystemConfig::default_enabled();
        assert!(config.enabled);
    }

    #[test]
    fn test_config_validation_success() {
        let config = SubAgentSystemConfig::default_enabled();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_depth() {
        let mut config = SubAgentSystemConfig::default_enabled();
        config.max_nesting_depth = 100;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_concurrent() {
        let mut config = SubAgentSystemConfig::default_enabled();
        config.max_concurrent = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_timeout() {
        let mut config = SubAgentSystemConfig::default_enabled();
        config.default_timeout_secs = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_spawn_config_builder() {
        let config = SubAgentSpawnConfig::new()
            .with_timeout(60)
            .with_max_iterations(10)
            .synchronous()
            .with_allowed_tools(vec!["tool_a".to_string(), "tool_b".to_string()]);

        assert_eq!(config.timeout_secs, Some(60));
        assert_eq!(config.max_iterations, Some(10));
        assert!(config.wait_for_completion);
        assert!(config.tool_access.allowed_tools.is_some());
    }

    #[test]
    fn test_tool_access_allow_all() {
        let access = SubAgentToolAccess::allow_all();
        assert!(access.allowed_tools.is_none());
        assert!(access.blocked_tools.is_empty());
    }

    #[test]
    fn test_tool_access_only() {
        let access = SubAgentToolAccess::only(vec!["tool_a".to_string()]);
        assert!(access.allowed_tools.is_some());
        assert!(access.allowed_tools.unwrap().contains("tool_a"));
    }

    #[test]
    fn test_tool_access_except() {
        let access = SubAgentToolAccess::except(vec!["dangerous_tool".to_string()]);
        assert!(access.allowed_tools.is_none());
        assert!(access.blocked_tools.contains("dangerous_tool"));
    }

    #[test]
    fn test_failure_policy_serialization() {
        let policy = FailurePolicy::Notify;
        let json = serde_json::to_string(&policy).unwrap();
        assert_eq!(json, "\"notify\"");

        let deserialized: FailurePolicy = serde_json::from_str("\"retry\"").unwrap();
        assert_eq!(deserialized, FailurePolicy::Retry);
    }

    #[test]
    fn test_failure_policy_behavior() {
        assert!(FailurePolicy::Retry.should_retry());
        assert!(!FailurePolicy::Fail.should_retry());

        assert!(FailurePolicy::Notify.should_notify());
        assert!(FailurePolicy::Fail.should_notify());
        assert!(!FailurePolicy::Ignore.should_notify());
    }

    #[test]
    fn test_config_serialization() {
        let config = SubAgentSystemConfig::default_enabled();
        let json = serde_json::to_string_pretty(&config).unwrap();

        assert!(json.contains("\"enabled\": true"));
        assert!(json.contains("\"max_nesting_depth\": 3"));

        let deserialized: SubAgentSystemConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.max_nesting_depth, 3);
    }

    #[test]
    fn test_default_blocked_tools() {
        let blocked = default_blocked_tools();
        assert!(blocked.contains("mcp__filesystem__delete_file"));
        assert!(blocked.contains("internal__spawn_sub_agent"));
    }

    // ========== Reflection Config Tests ==========

    #[test]
    fn test_reflection_config_default() {
        let config = SubAgentReflectionConfig::default();
        assert!(!config.enabled);
        assert_eq!(config.interval_iterations, 5);
        assert_eq!(config.min_iterations, 3);
        assert!(config.reflect_on_tool_error);
        assert!(config.reflect_on_completion);
        assert_eq!(config.confidence_threshold, 0.7);
        assert_eq!(config.max_retries, 2);
    }

    #[test]
    fn test_reflection_config_enabled() {
        let config = SubAgentReflectionConfig::enabled();
        assert!(config.enabled);
    }

    #[test]
    fn test_reflection_config_disabled() {
        let config = SubAgentReflectionConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_reflection_config_builder() {
        let config = SubAgentReflectionConfig::enabled()
            .with_interval(10)
            .with_min_iterations(5);

        assert!(config.enabled);
        assert_eq!(config.interval_iterations, 10);
        assert_eq!(config.min_iterations, 5);
    }

    #[test]
    fn test_should_reflect_at_iteration_disabled() {
        let config = SubAgentReflectionConfig::disabled();
        assert!(!config.should_reflect_at_iteration(5));
        assert!(!config.should_reflect_at_iteration(10));
    }

    #[test]
    fn test_should_reflect_at_iteration_enabled() {
        let config = SubAgentReflectionConfig::enabled()
            .with_interval(5)
            .with_min_iterations(3);

        // 低于最小迭代次数
        assert!(!config.should_reflect_at_iteration(1));
        assert!(!config.should_reflect_at_iteration(2));

        // 达到最小迭代次数但不是间隔倍数
        assert!(!config.should_reflect_at_iteration(3));
        assert!(!config.should_reflect_at_iteration(4));

        // 达到间隔倍数
        assert!(config.should_reflect_at_iteration(5));
        assert!(config.should_reflect_at_iteration(10));
        assert!(config.should_reflect_at_iteration(15));
    }

    #[test]
    fn test_should_reflect_at_iteration_zero_interval() {
        let config = SubAgentReflectionConfig {
            enabled: true,
            interval_iterations: 0, // 仅在完成时反思
            ..Default::default()
        };

        assert!(!config.should_reflect_at_iteration(5));
        assert!(!config.should_reflect_at_iteration(10));
    }

    #[test]
    fn test_should_reflect_on_completion_disabled() {
        let config = SubAgentReflectionConfig::disabled();
        assert!(!config.should_reflect_on_completion(10));
    }

    #[test]
    fn test_should_reflect_on_completion_enabled() {
        let config = SubAgentReflectionConfig::enabled().with_min_iterations(3);

        // 低于最小迭代次数
        assert!(!config.should_reflect_on_completion(1));
        assert!(!config.should_reflect_on_completion(2));

        // 达到最小迭代次数
        assert!(config.should_reflect_on_completion(3));
        assert!(config.should_reflect_on_completion(10));
    }

    #[test]
    fn test_should_reflect_on_completion_flag_disabled() {
        let config = SubAgentReflectionConfig {
            enabled: true,
            reflect_on_completion: false,
            min_iterations: 1,
            ..Default::default()
        };

        assert!(!config.should_reflect_on_completion(10));
    }

    #[test]
    fn test_reflection_config_in_system_config() {
        let mut config = SubAgentSystemConfig::default_enabled();
        config.reflection = SubAgentReflectionConfig::enabled().with_interval(10);

        assert!(config.reflection.enabled);
        assert_eq!(config.reflection.interval_iterations, 10);
    }

    #[test]
    fn test_reflection_config_serialization() {
        let config = SubAgentReflectionConfig::enabled()
            .with_interval(10)
            .with_min_iterations(5);

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("\"enabled\":true"));
        assert!(json.contains("\"interval_iterations\":10"));
        assert!(json.contains("\"min_iterations\":5"));

        let deserialized: SubAgentReflectionConfig = serde_json::from_str(&json).unwrap();
        assert!(deserialized.enabled);
        assert_eq!(deserialized.interval_iterations, 10);
        assert_eq!(deserialized.min_iterations, 5);
    }
}
