//! 风险评估器
//!
//! 实现三层风险合成策略：
//!
//! ```text
//! 最终风险 = max(L1 开发者声明, L2 系统推断) + L3 运行时修正
//! ```
//!
//! - L1: 开发者在 SKILL.md / MCP 配置中显式声明（不允许 Safe）
//! - L2: 基于工具名称关键词自动推断（兜底）
//! - L3: 用户批准后积累信任，逐级降低风险

use std::sync::Arc;

use super::{
    config::HitlConfig,
    risk_inference::{infer_risk_factors, infer_risk_from_name},
    trust_store::{TrustKey, TrustStore},
    types::{RiskFactor, RiskFactorSeverity, RiskLevel},
};

/// 风险评估结果
#[derive(Debug, Clone)]
pub struct RiskAssessment {
    /// 最终风险级别
    pub final_risk: RiskLevel,
    /// L1 开发者声明的风险级别（如有）
    pub l1_declared: Option<RiskLevel>,
    /// L2 系统推断的风险级别
    pub l2_inferred: RiskLevel,
    /// L3 运行时调整前的风险级别
    pub pre_l3_risk: RiskLevel,
    /// 是否需要确认
    pub requires_confirmation: bool,
    /// 风险因素列表
    pub risk_factors: Vec<RiskFactor>,
    /// 信任键（用于记录批准）
    pub trust_key: TrustKey,
    /// 当前批准次数
    pub approval_count: usize,
}

impl RiskAssessment {
    /// 获取有效的超时秒数
    pub fn get_timeout_secs(&self, config: &HitlConfig) -> u64 {
        config.get_timeout_secs(&self.trust_key.tool_name)
    }

    /// 获取有效的超时行为
    pub fn get_timeout_behavior(&self, config: &HitlConfig) -> super::types::TimeoutBehavior {
        config
            .get_timeout_behavior(&self.trust_key.tool_name)
            .effective_for_risk(self.final_risk)
    }
}

/// 风险评估器
pub struct RiskAssessor {
    config: HitlConfig,
    trust_store: Arc<TrustStore>,
}

impl RiskAssessor {
    /// 创建新的风险评估器
    pub fn new(config: HitlConfig, trust_store: Arc<TrustStore>) -> Self {
        Self {
            config,
            trust_store,
        }
    }

    /// 评估工具调用的风险
    ///
    /// 实现三层风险合成策略：
    /// 1. L1 开发者声明（从配置中获取）
    /// 2. L2 系统推断（基于工具名称）
    /// 3. L3 运行时调整（基于用户批准历史）
    ///
    /// 公式：`最终风险 = max(L1, L2) + L3`
    pub fn assess(&self, tool_name: &str, args: &serde_json::Value) -> RiskAssessment {
        // L1: 开发者声明
        let l1_declared = self
            .config
            .get_tool_override(tool_name)
            .map(|d| d.risk_level);

        // L2: 系统推断
        let l2_inferred = infer_risk_from_name(tool_name);

        // 计算 L1/L2 合成结果
        let pre_l3_risk = match l1_declared {
            Some(l1) => l1.max(l2_inferred), // max(L1, L2)
            None => l2_inferred,
        };

        // L3: 运行时调整
        let trust_key = TrustKey::from_tool_call(tool_name, args);
        let approval_count = self.trust_store.get_approval_count(&trust_key);

        let final_risk = if let Some(learning_config) = &self.config.runtime_learning {
            self.trust_store
                .compute_adjustment(&trust_key, pre_l3_risk, learning_config)
        } else {
            pre_l3_risk
        };

        // 收集风险因素
        let risk_factors = self.collect_risk_factors(tool_name, args, &l1_declared, l2_inferred);

        // 判断是否需要确认
        let requires_confirmation = self.config.enabled
            && final_risk.requires_confirmation(self.config.confirmation_threshold);

        RiskAssessment {
            final_risk,
            l1_declared,
            l2_inferred,
            pre_l3_risk,
            requires_confirmation,
            risk_factors,
            trust_key,
            approval_count,
        }
    }

    /// 评估 MCP 工具的风险
    pub fn assess_mcp_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> RiskAssessment {
        let full_name = format!("mcp__{}_{}", server_name, tool_name);
        self.assess(&full_name, args)
    }

    /// 记录用户批准
    ///
    /// 在用户批准工具调用后调用此方法，用于 L3 信任积累
    pub fn record_approval(&self, tool_name: &str, args: &serde_json::Value) {
        if let Some(learning_config) = &self.config.runtime_learning {
            let trust_key = TrustKey::from_tool_call(tool_name, args);
            self.trust_store.record_approval(trust_key, learning_config);
        }
    }

    /// 记录批准（使用已有的 TrustKey）
    pub fn record_approval_with_key(&self, trust_key: TrustKey) {
        if let Some(learning_config) = &self.config.runtime_learning {
            self.trust_store.record_approval(trust_key, learning_config);
        }
    }

    /// 收集风险因素
    fn collect_risk_factors(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        l1_declared: &Option<RiskLevel>,
        l2_inferred: RiskLevel,
    ) -> Vec<RiskFactor> {
        let mut factors = Vec::new();

        // L2 推断产生的因素
        let inferred_factors = infer_risk_factors(tool_name, args);
        for f in inferred_factors {
            factors.push(RiskFactor {
                code: f.code,
                description: f.description,
                severity: if f.risk_increase {
                    RiskFactorSeverity::Warning
                } else {
                    RiskFactorSeverity::Info
                },
            });
        }

        // L1 声明高于 L2 推断
        if let Some(l1) = l1_declared
            && *l1 > l2_inferred
        {
            factors.push(RiskFactor {
                code: "l1_elevated".to_string(),
                description: format!(
                    "开发者声明此工具风险为 {:?}（高于系统推断的 {:?}）",
                    l1, l2_inferred
                ),
                severity: RiskFactorSeverity::Warning,
            });
        }

        // Critical 级别特殊警告
        if l1_declared == &Some(RiskLevel::Critical) || l2_inferred == RiskLevel::Critical {
            factors.push(RiskFactor {
                code: "critical_operation".to_string(),
                description: "这是一个关键操作，请仔细确认".to_string(),
                severity: RiskFactorSeverity::Danger,
            });
        }

        factors
    }

    /// 获取配置
    pub fn config(&self) -> &HitlConfig {
        &self.config
    }

    /// 获取信任存储
    pub fn trust_store(&self) -> &Arc<TrustStore> {
        &self.trust_store
    }

    /// 清理过期的信任记录
    pub fn cleanup_expired_trust(&self) -> usize {
        self.trust_store.cleanup_expired()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::*;
    use crate::services::hitl::config::{DeclaredRisk, RuntimeLearningConfig};

    fn create_test_config() -> HitlConfig {
        HitlConfig {
            enabled: true,
            default_timeout_secs: 300,
            default_timeout_behavior: super::super::types::TimeoutBehavior::Reject,
            confirmation_threshold: RiskLevel::Medium,
            tool_overrides: HashMap::new(),
            runtime_learning: None,
        }
    }

    fn create_test_config_with_learning() -> HitlConfig {
        HitlConfig {
            enabled: true,
            default_timeout_secs: 300,
            default_timeout_behavior: super::super::types::TimeoutBehavior::Reject,
            confirmation_threshold: RiskLevel::Medium,
            tool_overrides: HashMap::new(),
            runtime_learning: Some(RuntimeLearningConfig {
                approvals_per_level: 3,
                trust_duration_secs: 604800,
                min_level: RiskLevel::Low,
            }),
        }
    }

    #[test]
    fn test_assess_low_risk_tool() {
        let config = create_test_config();
        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess("read_file", &json!({ "path": "/tmp/test.txt" }));

        assert_eq!(assessment.final_risk, RiskLevel::Low);
        assert_eq!(assessment.l2_inferred, RiskLevel::Low);
        assert!(!assessment.requires_confirmation);
    }

    #[test]
    fn test_assess_high_risk_tool() {
        let config = create_test_config();
        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess("delete_file", &json!({ "path": "/tmp/test.txt" }));

        assert_eq!(assessment.final_risk, RiskLevel::High);
        assert_eq!(assessment.l2_inferred, RiskLevel::High);
        assert!(assessment.requires_confirmation);
    }

    #[test]
    fn test_assess_with_l1_override() {
        let mut config = create_test_config();
        config.tool_overrides.insert(
            "read_file".to_string(),
            DeclaredRisk::new(RiskLevel::High), // 将读取文件声明为高风险
        );

        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess("read_file", &json!({ "path": "/tmp/test.txt" }));

        assert_eq!(assessment.l1_declared, Some(RiskLevel::High));
        assert_eq!(assessment.l2_inferred, RiskLevel::Low);
        assert_eq!(assessment.final_risk, RiskLevel::High); // max(High, Low) = High
        assert!(assessment.requires_confirmation);
    }

    #[test]
    fn test_assess_l1_l2_max() {
        let mut config = create_test_config();
        // L1 声明为 Medium
        config.tool_overrides.insert(
            "send_email".to_string(),
            DeclaredRisk::new(RiskLevel::Medium),
        );

        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess("send_email", &json!({}));

        assert_eq!(assessment.l1_declared, Some(RiskLevel::Medium));
        assert_eq!(assessment.l2_inferred, RiskLevel::High); // 包含 "send"
        assert_eq!(assessment.final_risk, RiskLevel::High); // max(Medium, High) = High
    }

    #[test]
    fn test_assess_with_l3_adjustment() {
        let config = create_test_config_with_learning();
        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store.clone());

        // 初始评估
        let assessment = assessor.assess("delete_file", &json!({ "path": "/tmp/test.txt" }));
        assert_eq!(assessment.final_risk, RiskLevel::High);

        // 记录 3 次批准
        for _ in 0..3 {
            assessor.record_approval("delete_file", &json!({ "path": "/tmp/test.txt" }));
        }

        // 再次评估，应该降低一级
        let assessment = assessor.assess("delete_file", &json!({ "path": "/tmp/test.txt" }));
        assert_eq!(assessment.final_risk, RiskLevel::Medium);
        assert_eq!(assessment.approval_count, 3);
    }

    #[test]
    fn test_assess_different_paths() {
        let config = create_test_config_with_learning();
        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        // 在 /tmp 路径记录批准
        for _ in 0..3 {
            assessor.record_approval("delete_file", &json!({ "path": "/tmp/test.txt" }));
        }

        // /tmp 路径应该降级
        let assessment = assessor.assess("delete_file", &json!({ "path": "/tmp/other.txt" }));
        assert_eq!(assessment.final_risk, RiskLevel::Medium);

        // /etc 路径不应该降级（不同的 TrustKey）
        let assessment = assessor.assess("delete_file", &json!({ "path": "/etc/passwd" }));
        assert_eq!(assessment.final_risk, RiskLevel::High);
    }

    #[test]
    fn test_assess_mcp_tool() {
        let config = create_test_config();
        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess_mcp_tool("email", "send_email", &json!({}));

        assert_eq!(assessment.final_risk, RiskLevel::High);
        assert!(assessment.trust_key.tool_name.contains("mcp__email"));
    }

    #[test]
    fn test_confirmation_threshold() {
        let mut config = create_test_config();
        config.confirmation_threshold = RiskLevel::High;

        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        // Low 风险工具不需要确认（阈值是 High）
        let assessment = assessor.assess("read_config", &json!({}));
        assert!(!assessment.requires_confirmation);
        assert_eq!(assessment.l2_inferred, RiskLevel::Low);

        // High 需要确认
        let assessment = assessor.assess("delete_file", &json!({}));
        assert!(assessment.requires_confirmation);
        assert_eq!(assessment.l2_inferred, RiskLevel::High);
    }

    #[test]
    fn test_disabled_hitl() {
        let mut config = create_test_config();
        config.enabled = false;

        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        // 即使是高风险操作，禁用后也不需要确认
        let assessment = assessor.assess("delete_file", &json!({}));
        assert!(!assessment.requires_confirmation);
    }

    #[test]
    fn test_risk_factors_collection() {
        let mut config = create_test_config();
        config.tool_overrides.insert(
            "api_call".to_string(),
            DeclaredRisk::new(RiskLevel::Critical),
        );

        let trust_store = Arc::new(TrustStore::new());
        let assessor = RiskAssessor::new(config, trust_store);

        let assessment = assessor.assess("api_call", &json!({ "url": "https://external.com/api" }));

        // 应该有 critical_operation 因素
        assert!(
            assessment
                .risk_factors
                .iter()
                .any(|f| f.code == "critical_operation")
        );

        // 应该有 external_resource 因素
        assert!(
            assessment
                .risk_factors
                .iter()
                .any(|f| f.code == "external_resource")
        );
    }
}
