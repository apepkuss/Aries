//! HITL 风险配置
//!
//! 本模块定义了 HITL 机制的配置结构，包括：
//! - `HitlConfig`: 全局配置
//! - `DeclaredRisk`: L1 开发者声明的风险配置
//! - `RuntimeLearningConfig`: L3 运行时学习配置

use std::collections::HashMap;

use serde::{Deserialize, Deserializer, Serialize};

use super::types::{RiskLevel, TimeoutBehavior};

/// HITL 风险配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlConfig {
    /// 是否启用 HITL
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// 全局默认超时秒数
    #[serde(default = "default_timeout_secs")]
    pub default_timeout_secs: u64,

    /// 全局默认超时行为
    #[serde(default)]
    pub default_timeout_behavior: TimeoutBehavior,

    /// 需要确认的最低风险级别（>= 此级别才触发确认）
    #[serde(default = "default_confirmation_threshold")]
    pub confirmation_threshold: RiskLevel,

    /// L1: 开发者显式声明的工具风险（覆盖 L2 推断）
    #[serde(default)]
    pub tool_overrides: HashMap<String, DeclaredRisk>,

    /// L3: 运行时修正配置（可选）
    #[serde(default)]
    pub runtime_learning: Option<RuntimeLearningConfig>,
}

fn default_enabled() -> bool {
    true
}

fn default_timeout_secs() -> u64 {
    300 // 5 分钟
}

fn default_confirmation_threshold() -> RiskLevel {
    RiskLevel::Medium
}

impl Default for HitlConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            default_timeout_secs: default_timeout_secs(),
            default_timeout_behavior: TimeoutBehavior::default(),
            confirmation_threshold: default_confirmation_threshold(),
            tool_overrides: HashMap::new(),
            runtime_learning: None,
        }
    }
}

impl HitlConfig {
    /// 创建禁用状态的配置
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    /// 获取工具的声明风险配置
    pub fn get_tool_override(&self, tool_name: &str) -> Option<&DeclaredRisk> {
        self.tool_overrides.get(tool_name)
    }

    /// 获取工具的有效超时秒数
    pub fn get_timeout_secs(&self, tool_name: &str) -> u64 {
        self.tool_overrides
            .get(tool_name)
            .map(|d| d.timeout_secs)
            .unwrap_or(self.default_timeout_secs)
    }

    /// 获取工具的有效超时行为
    pub fn get_timeout_behavior(&self, tool_name: &str) -> TimeoutBehavior {
        self.tool_overrides
            .get(tool_name)
            .map(|d| d.timeout_behavior)
            .unwrap_or(self.default_timeout_behavior)
    }

    /// 检查是否启用运行时学习
    pub fn is_learning_enabled(&self) -> bool {
        self.runtime_learning.is_some()
    }
}

/// 开发者声明的风险配置（L1）
///
/// 注意：`risk_level` 只允许 Low, Medium, High, Critical，不允许 Safe。
/// Safe 只能通过 L3 运行时信任获得。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeclaredRisk {
    /// 风险级别（不允许 Safe）
    #[serde(deserialize_with = "deserialize_non_safe_risk_level")]
    pub risk_level: RiskLevel,

    /// 超时秒数
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,

    /// 超时行为
    #[serde(default)]
    pub timeout_behavior: TimeoutBehavior,
}

impl DeclaredRisk {
    /// 创建新的声明风险配置
    ///
    /// # Panics
    /// 如果 `risk_level` 为 `Safe`，则会 panic。
    pub fn new(risk_level: RiskLevel) -> Self {
        assert!(
            risk_level.is_declarable(),
            "L1 声明不允许 Safe 级别，Safe 只能通过 L3 运行时信任获得"
        );

        Self {
            risk_level,
            timeout_secs: default_timeout_secs(),
            timeout_behavior: TimeoutBehavior::default(),
        }
    }

    /// 创建新的声明风险配置（可能失败）
    pub fn try_new(risk_level: RiskLevel) -> Result<Self, &'static str> {
        if !risk_level.is_declarable() {
            return Err("L1 声明不允许 Safe 级别，Safe 只能通过 L3 运行时信任获得");
        }

        Ok(Self {
            risk_level,
            timeout_secs: default_timeout_secs(),
            timeout_behavior: TimeoutBehavior::default(),
        })
    }

    /// 设置超时秒数
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    /// 设置超时行为
    pub fn with_timeout_behavior(mut self, behavior: TimeoutBehavior) -> Self {
        self.timeout_behavior = behavior;
        self
    }
}

/// 自定义反序列化：拒绝 Safe 级别的声明
fn deserialize_non_safe_risk_level<'de, D>(deserializer: D) -> Result<RiskLevel, D::Error>
where
    D: Deserializer<'de>,
{
    let level = RiskLevel::deserialize(deserializer)?;
    if level == RiskLevel::Safe {
        return Err(serde::de::Error::custom(
            "L1 声明不允许 Safe 级别，Safe 只能通过 L3 运行时信任获得",
        ));
    }
    Ok(level)
}

/// 运行时学习配置（L3）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeLearningConfig {
    /// 连续批准 N 次后可降低一级风险
    #[serde(default = "default_approvals_per_level")]
    pub approvals_per_level: usize,

    /// 信任有效期（秒），0 表示永久
    #[serde(default = "default_trust_duration_secs")]
    pub trust_duration_secs: u64,

    /// 最低可降级到的级别（防止 Critical 工具降到 Safe）
    #[serde(default = "default_min_level")]
    pub min_level: RiskLevel,
}

fn default_approvals_per_level() -> usize {
    3
}

fn default_trust_duration_secs() -> u64 {
    604800 // 7 天
}

fn default_min_level() -> RiskLevel {
    RiskLevel::Low
}

impl Default for RuntimeLearningConfig {
    fn default() -> Self {
        Self {
            approvals_per_level: default_approvals_per_level(),
            trust_duration_secs: default_trust_duration_secs(),
            min_level: default_min_level(),
        }
    }
}

impl RuntimeLearningConfig {
    /// 计算给定批准次数可以降低的级别数
    pub fn levels_to_reduce(&self, approval_count: usize) -> usize {
        if self.approvals_per_level == 0 {
            return 0;
        }
        approval_count / self.approvals_per_level
    }

    /// 检查是否启用永久信任
    pub fn is_permanent_trust(&self) -> bool {
        self.trust_duration_secs == 0
    }
}

/// 操作配置（运维相关）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HitlOpsConfig {
    /// 最大待处理请求数
    #[serde(default = "default_max_pending")]
    pub max_pending_requests: usize,

    /// 超时检查间隔秒数
    #[serde(default = "default_timeout_check_interval")]
    pub timeout_check_interval_secs: u64,

    /// 超时警告提前秒数
    #[serde(default = "default_timeout_warning_secs")]
    pub timeout_warning_before_secs: u64,

    /// 是否启用历史记录
    #[serde(default)]
    pub history_enabled: bool,

    /// 历史记录保留天数
    #[serde(default = "default_history_retention_days")]
    pub history_retention_days: u32,
}

fn default_max_pending() -> usize {
    100
}

fn default_timeout_check_interval() -> u64 {
    1
}

fn default_timeout_warning_secs() -> u64 {
    30
}

fn default_history_retention_days() -> u32 {
    90
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hitl_config_default() {
        let config = HitlConfig::default();

        assert!(config.enabled);
        assert_eq!(config.default_timeout_secs, 300);
        assert_eq!(config.default_timeout_behavior, TimeoutBehavior::Reject);
        assert_eq!(config.confirmation_threshold, RiskLevel::Medium);
        assert!(config.tool_overrides.is_empty());
        assert!(config.runtime_learning.is_none());
    }

    #[test]
    fn test_hitl_config_disabled() {
        let config = HitlConfig::disabled();
        assert!(!config.enabled);
    }

    #[test]
    fn test_declared_risk_new() {
        let risk = DeclaredRisk::new(RiskLevel::High);
        assert_eq!(risk.risk_level, RiskLevel::High);
        assert_eq!(risk.timeout_secs, 300);
        assert_eq!(risk.timeout_behavior, TimeoutBehavior::Reject);
    }

    #[test]
    #[should_panic(expected = "L1 声明不允许 Safe 级别")]
    fn test_declared_risk_reject_safe() {
        DeclaredRisk::new(RiskLevel::Safe);
    }

    #[test]
    fn test_declared_risk_try_new() {
        assert!(DeclaredRisk::try_new(RiskLevel::High).is_ok());
        assert!(DeclaredRisk::try_new(RiskLevel::Safe).is_err());
    }

    #[test]
    fn test_declared_risk_deserialize_reject_safe() {
        let json = r#"{"risk_level": "safe", "timeout_secs": 60}"#;
        let result: Result<DeclaredRisk, _> = serde_json::from_str(json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Safe"));
    }

    #[test]
    fn test_declared_risk_deserialize_accept_high() {
        let json = r#"{"risk_level": "high", "timeout_secs": 60}"#;
        let result: DeclaredRisk = serde_json::from_str(json).unwrap();
        assert_eq!(result.risk_level, RiskLevel::High);
        assert_eq!(result.timeout_secs, 60);
    }

    #[test]
    fn test_runtime_learning_config_default() {
        let config = RuntimeLearningConfig::default();

        assert_eq!(config.approvals_per_level, 3);
        assert_eq!(config.trust_duration_secs, 604800);
        assert_eq!(config.min_level, RiskLevel::Low);
    }

    #[test]
    fn test_runtime_learning_levels_to_reduce() {
        let config = RuntimeLearningConfig {
            approvals_per_level: 3,
            ..Default::default()
        };

        assert_eq!(config.levels_to_reduce(0), 0);
        assert_eq!(config.levels_to_reduce(1), 0);
        assert_eq!(config.levels_to_reduce(2), 0);
        assert_eq!(config.levels_to_reduce(3), 1);
        assert_eq!(config.levels_to_reduce(5), 1);
        assert_eq!(config.levels_to_reduce(6), 2);
        assert_eq!(config.levels_to_reduce(9), 3);
    }

    #[test]
    fn test_hitl_config_get_tool_override() {
        let mut config = HitlConfig::default();
        config.tool_overrides.insert(
            "dangerous_tool".to_string(),
            DeclaredRisk::new(RiskLevel::Critical).with_timeout_secs(600),
        );

        assert!(config.get_tool_override("unknown_tool").is_none());

        let override_config = config.get_tool_override("dangerous_tool").unwrap();
        assert_eq!(override_config.risk_level, RiskLevel::Critical);
        assert_eq!(override_config.timeout_secs, 600);
    }

    #[test]
    fn test_hitl_config_get_timeout() {
        let mut config = HitlConfig::default();
        config.default_timeout_secs = 120;
        config.tool_overrides.insert(
            "slow_tool".to_string(),
            DeclaredRisk::new(RiskLevel::Medium).with_timeout_secs(600),
        );

        // 未配置的工具使用默认值
        assert_eq!(config.get_timeout_secs("unknown_tool"), 120);

        // 已配置的工具使用自定义值
        assert_eq!(config.get_timeout_secs("slow_tool"), 600);
    }

    #[test]
    fn test_hitl_config_serialization() {
        let config = HitlConfig {
            enabled: true,
            default_timeout_secs: 180,
            default_timeout_behavior: TimeoutBehavior::Skip,
            confirmation_threshold: RiskLevel::High,
            tool_overrides: HashMap::new(),
            runtime_learning: Some(RuntimeLearningConfig::default()),
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: HitlConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.enabled, config.enabled);
        assert_eq!(parsed.default_timeout_secs, config.default_timeout_secs);
        assert_eq!(
            parsed.default_timeout_behavior,
            config.default_timeout_behavior
        );
        assert_eq!(parsed.confirmation_threshold, config.confirmation_threshold);
        assert!(parsed.runtime_learning.is_some());
    }
}
