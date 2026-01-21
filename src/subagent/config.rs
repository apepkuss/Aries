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

        Ok(())
    }
}

impl Default for SubAgentSystemConfig {
    fn default() -> Self {
        Self {
            enabled: false,
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
        }
    }
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
    /// 立即失败，将错误传播给主 Agent
    #[default]
    Fail,
    /// 通知主 Agent，让其决定如何处理
    Notify,
    /// 自动重试
    Retry,
    /// 忽略失败，继续执行
    Ignore,
}

impl FailurePolicy {
    /// 是否需要重试
    pub fn should_retry(&self) -> bool {
        matches!(self, FailurePolicy::Retry)
    }

    /// 是否需要通知主 Agent
    pub fn should_notify(&self) -> bool {
        matches!(self, FailurePolicy::Notify | FailurePolicy::Fail)
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
}
