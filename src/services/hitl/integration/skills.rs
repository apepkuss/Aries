//! Skills HITL 集成
//!
//! 提供 Agent Skills 与 HITL 机制的集成支持。
//!
//! # 功能
//!
//! - 从 SkillMetadata 读取 HITL 配置
//! - 技能级别的风险声明（L1）
//! - 技能工具的 HITL 包装
//!
//! # 配置
//!
//! Skills 可以通过 SKILL.md 的 metadata 字段声明 HITL 配置：
//!
//! ```yaml
//! metadata:
//!   hitl:
//!     risk_level: high       # 技能整体风险级别
//!     require_confirmation:  # 需要确认的工具列表
//!       - shell_execute
//!       - file_write
//!     trusted_tools:         # 信任的工具列表（跳过确认）
//!       - read_file
//!       - list_directory
//!     timeout_secs: 300      # 确认超时秒数
//! ```
//!
//! # 使用示例
//!
//! ```ignore
//! use crate::services::hitl::integration::skills::SkillHitlAdapter;
//!
//! let adapter = SkillHitlAdapter::new(hitl_manager.clone());
//!
//! // 从技能元数据创建配置
//! let config = adapter.config_from_metadata(&skill_metadata);
//!
//! // 检查技能工具是否需要确认
//! let needs_confirm = adapter.needs_confirmation(&skill, "shell_execute", &args);
//! ```

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use super::tool_caller::{HitlToolCaller, HitlToolContext, HitlToolResult};
use crate::{
    services::hitl::{
        config::DeclaredRisk,
        manager::HitlManager,
        risk_assessor::RiskAssessment,
        types::{HitlError, RiskLevel},
    },
    skills::types::SkillMetadata,
};

/// 技能 HITL 配置
///
/// 从 SkillMetadata 解析得到的 HITL 配置
#[derive(Debug, Clone, Default)]
pub struct SkillHitlConfig {
    /// 技能整体风险级别
    pub risk_level: Option<RiskLevel>,
    /// 需要确认的工具列表
    pub require_confirmation: HashSet<String>,
    /// 信任的工具列表（跳过确认）
    pub trusted_tools: HashSet<String>,
    /// 确认超时秒数
    pub timeout_secs: Option<u64>,
    /// 工具级别的风险覆盖
    pub tool_risk_levels: HashMap<String, RiskLevel>,
}

impl SkillHitlConfig {
    /// 从 SkillMetadata 解析 HITL 配置
    pub fn from_metadata(metadata: &SkillMetadata) -> Self {
        let mut config = Self::default();

        // 从 metadata 中解析 hitl 配置
        if let Some(ref meta_val) = metadata.metadata
            && let Some(meta_map) = meta_val.as_object()
        {
            // 解析风险级别
            if let Some(level_str) = meta_map.get("hitl.risk_level").and_then(|v| v.as_str()) {
                config.risk_level = Self::parse_risk_level(level_str);
            }

            // 解析需要确认的工具
            if let Some(tools_str) = meta_map
                .get("hitl.require_confirmation")
                .and_then(|v| v.as_str())
            {
                config.require_confirmation = Self::parse_tool_list(tools_str);
            }

            // 解析信任的工具
            if let Some(tools_str) = meta_map.get("hitl.trusted_tools").and_then(|v| v.as_str()) {
                config.trusted_tools = Self::parse_tool_list(tools_str);
            }

            // 解析超时秒数
            if let Some(timeout_str) = meta_map.get("hitl.timeout_secs").and_then(|v| v.as_str()) {
                config.timeout_secs = timeout_str.parse().ok();
            }

            // 解析工具级别的风险覆盖
            for (key, value) in meta_map {
                if let Some(tool_name) = key.strip_prefix("hitl.tool_risk.")
                    && let Some(val_str) = value.as_str()
                    && let Some(level) = Self::parse_risk_level(val_str)
                {
                    config.tool_risk_levels.insert(tool_name.to_string(), level);
                }
            }
        }

        config
    }

    /// 解析风险级别字符串
    fn parse_risk_level(s: &str) -> Option<RiskLevel> {
        match s.to_lowercase().as_str() {
            "low" => Some(RiskLevel::Low),
            "medium" => Some(RiskLevel::Medium),
            "high" => Some(RiskLevel::High),
            "critical" => Some(RiskLevel::Critical),
            _ => None,
        }
    }

    /// 解析工具列表字符串
    fn parse_tool_list(s: &str) -> HashSet<String> {
        s.split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// 检查工具是否需要确认
    pub fn needs_confirmation(&self, tool_name: &str) -> Option<bool> {
        // 如果在信任列表中，不需要确认
        if self.trusted_tools.contains(tool_name) {
            return Some(false);
        }

        // 如果在需要确认列表中，需要确认
        if self.require_confirmation.contains(tool_name) {
            return Some(true);
        }

        // 返回 None 表示使用默认策略
        None
    }

    /// 获取工具的风险级别
    pub fn get_tool_risk_level(&self, tool_name: &str) -> Option<RiskLevel> {
        // 首先检查工具级别覆盖
        if let Some(level) = self.tool_risk_levels.get(tool_name) {
            return Some(*level);
        }

        // 返回技能整体风险级别
        self.risk_level
    }

    /// 转换为工具覆盖配置（用于 L1 配置）
    ///
    /// 返回 (工具模式, DeclaredRisk) 的 HashMap，可以合并到 HitlConfig.tool_overrides
    pub fn to_tool_overrides(&self, skill_name: &str) -> HashMap<String, DeclaredRisk> {
        let mut overrides = HashMap::new();

        // 添加技能整体风险声明（使用通配符模式）
        if let Some(level) = self.risk_level {
            let pattern = format!("skill:{}:*", skill_name);
            overrides.insert(
                pattern,
                DeclaredRisk::new(level).with_timeout_secs(self.timeout_secs.unwrap_or(300)),
            );
        }

        // 添加工具级别风险声明
        for (tool_name, level) in &self.tool_risk_levels {
            let pattern = format!("skill:{}:{}", skill_name, tool_name);
            overrides.insert(
                pattern,
                DeclaredRisk::new(*level).with_timeout_secs(self.timeout_secs.unwrap_or(300)),
            );
        }

        overrides
    }
}

/// 技能 HITL 适配器
///
/// 为 Agent Skills 提供 HITL 集成支持。
pub struct SkillHitlAdapter {
    caller: HitlToolCaller,
    manager: Arc<HitlManager>,
    /// 技能配置缓存
    skill_configs: std::sync::RwLock<HashMap<String, SkillHitlConfig>>,
}

impl SkillHitlAdapter {
    /// 创建新的技能 HITL 适配器
    pub fn new(manager: Arc<HitlManager>) -> Self {
        let caller = HitlToolCaller::new(manager.clone());
        Self {
            caller,
            manager,
            skill_configs: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// 获取内部的 HitlToolCaller
    pub fn caller(&self) -> &HitlToolCaller {
        &self.caller
    }

    /// 从技能元数据创建并缓存配置
    pub fn register_skill(&self, metadata: &SkillMetadata) {
        let config = SkillHitlConfig::from_metadata(metadata);
        if let Ok(mut configs) = self.skill_configs.write() {
            configs.insert(metadata.name.clone(), config);
        }
    }

    /// 获取技能的 HITL 配置
    pub fn get_skill_config(&self, skill_name: &str) -> Option<SkillHitlConfig> {
        self.skill_configs
            .read()
            .ok()
            .and_then(|configs| configs.get(skill_name).cloned())
    }

    /// 从技能元数据获取配置
    pub fn config_from_metadata(&self, metadata: &SkillMetadata) -> SkillHitlConfig {
        SkillHitlConfig::from_metadata(metadata)
    }

    /// 评估技能工具的风险
    ///
    /// # 参数
    /// - `skill_name`: 技能名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    ///
    /// # 返回
    /// 风险评估结果
    pub fn assess_risk(
        &self,
        skill_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> RiskAssessment {
        // 使用技能:工具格式的名称进行评估
        let full_name = format!("skill:{}:{}", skill_name, tool_name);
        self.manager.assess_risk(&full_name, args)
    }

    /// 检查技能工具是否需要 HITL 确认
    ///
    /// # 参数
    /// - `skill_name`: 技能名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    ///
    /// # 返回
    /// 如果需要确认返回 true
    pub fn needs_confirmation(
        &self,
        skill_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> bool {
        // 首先检查技能配置
        if let Some(config) = self.get_skill_config(skill_name)
            && let Some(needs) = config.needs_confirmation(tool_name)
        {
            return needs;
        }

        // 回退到通用风险评估
        self.assess_risk(skill_name, tool_name, args)
            .requires_confirmation
    }

    /// 使用 HITL 包装执行技能工具
    ///
    /// # 参数
    /// - `skill_name`: 技能名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    /// - `context`: HITL 上下文
    /// - `execute_fn`: 工具执行函数
    ///
    /// # 返回
    /// 执行结果
    pub async fn execute_with_hitl<T, F, Fut>(
        &self,
        skill_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let full_name = format!("skill:{}:{}", skill_name, tool_name);
        self.caller
            .check_and_execute(&full_name, args, context, execute_fn)
            .await
    }

    /// 获取技能工具的风险级别
    pub fn get_risk_level(
        &self,
        skill_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> RiskLevel {
        // 首先检查技能配置
        if let Some(config) = self.get_skill_config(skill_name)
            && let Some(level) = config.get_tool_risk_level(tool_name)
        {
            return level;
        }

        // 回退到通用风险评估
        self.assess_risk(skill_name, tool_name, args).final_risk
    }
}

/// 技能内部工具的 HITL 配置
///
/// 用于配置技能中的脚本执行等内部工具
#[derive(Debug, Clone, Default)]
pub struct SkillInternalToolConfig {
    /// 脚本执行风险级别
    pub script_execution_risk: RiskLevel,
    /// 资源访问风险级别
    pub resource_access_risk: RiskLevel,
    /// 需要确认的脚本模式
    pub require_confirmation_patterns: Vec<String>,
    /// 信任的脚本模式
    pub trusted_script_patterns: Vec<String>,
}

impl SkillInternalToolConfig {
    /// 检查脚本是否需要确认
    pub fn needs_confirmation(&self, script_path: &str) -> bool {
        // 检查信任模式
        for pattern in &self.trusted_script_patterns {
            if glob_match(pattern, script_path) {
                return false;
            }
        }

        // 检查需要确认的模式
        for pattern in &self.require_confirmation_patterns {
            if glob_match(pattern, script_path) {
                return true;
            }
        }

        // 默认根据风险级别决定
        self.script_execution_risk >= RiskLevel::Medium
    }
}

/// 简单的 glob 模式匹配
fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            let (prefix, suffix) = (parts[0], parts[1]);
            return text.starts_with(prefix) && text.ends_with(suffix);
        }
    }
    pattern == text
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::services::hitl::config::HitlConfig;

    fn create_test_manager() -> Arc<HitlManager> {
        let mut config = HitlConfig::default();
        config.enabled = true;
        Arc::new(HitlManager::new(config, 100))
    }

    fn create_test_metadata() -> SkillMetadata {
        SkillMetadata {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            license: None,
            compatibility: None,
            metadata: Some(serde_json::json!({
                "hitl.risk_level": "high",
                "hitl.require_confirmation": "shell_execute, file_write",
                "hitl.trusted_tools": "read_file, list_dir",
                "hitl.timeout_secs": "300",
                "hitl.tool_risk.delete_file": "critical"
            })),
            allowed_tools: None,
            model: None,
            parameters: None,
        }
    }

    #[test]
    fn test_skill_hitl_config_from_metadata() {
        let metadata = create_test_metadata();
        let config = SkillHitlConfig::from_metadata(&metadata);

        assert_eq!(config.risk_level, Some(RiskLevel::High));
        assert!(config.require_confirmation.contains("shell_execute"));
        assert!(config.require_confirmation.contains("file_write"));
        assert!(config.trusted_tools.contains("read_file"));
        assert!(config.trusted_tools.contains("list_dir"));
        assert_eq!(config.timeout_secs, Some(300));
        assert_eq!(
            config.tool_risk_levels.get("delete_file"),
            Some(&RiskLevel::Critical)
        );
    }

    #[test]
    fn test_skill_hitl_config_needs_confirmation() {
        let metadata = create_test_metadata();
        let config = SkillHitlConfig::from_metadata(&metadata);

        // 信任的工具不需要确认
        assert_eq!(config.needs_confirmation("read_file"), Some(false));
        assert_eq!(config.needs_confirmation("list_dir"), Some(false));

        // 需要确认的工具需要确认
        assert_eq!(config.needs_confirmation("shell_execute"), Some(true));
        assert_eq!(config.needs_confirmation("file_write"), Some(true));

        // 未指定的工具返回 None
        assert_eq!(config.needs_confirmation("unknown_tool"), None);
    }

    #[test]
    fn test_skill_hitl_config_get_tool_risk_level() {
        let metadata = create_test_metadata();
        let config = SkillHitlConfig::from_metadata(&metadata);

        // 工具级别覆盖
        assert_eq!(
            config.get_tool_risk_level("delete_file"),
            Some(RiskLevel::Critical)
        );

        // 回退到整体风险级别
        assert_eq!(
            config.get_tool_risk_level("unknown_tool"),
            Some(RiskLevel::High)
        );
    }

    #[test]
    fn test_skill_hitl_config_to_tool_overrides() {
        let metadata = create_test_metadata();
        let config = SkillHitlConfig::from_metadata(&metadata);
        let overrides = config.to_tool_overrides("test-skill");

        assert!(!overrides.is_empty());

        // 检查整体风险声明
        let skill_key = "skill:test-skill:*";
        let skill_risk = overrides.get(skill_key);
        assert!(skill_risk.is_some());
        assert_eq!(skill_risk.unwrap().risk_level, RiskLevel::High);

        // 检查工具级别风险声明
        let tool_key = "skill:test-skill:delete_file";
        let tool_risk = overrides.get(tool_key);
        assert!(tool_risk.is_some());
        assert_eq!(tool_risk.unwrap().risk_level, RiskLevel::Critical);
    }

    #[test]
    fn test_skill_hitl_adapter_register_and_get() {
        let manager = create_test_manager();
        let adapter = SkillHitlAdapter::new(manager);
        let metadata = create_test_metadata();

        adapter.register_skill(&metadata);

        let config = adapter.get_skill_config("test-skill");
        assert!(config.is_some());
        assert_eq!(config.unwrap().risk_level, Some(RiskLevel::High));
    }

    #[test]
    fn test_skill_hitl_adapter_needs_confirmation() {
        let manager = create_test_manager();
        let adapter = SkillHitlAdapter::new(manager);
        let metadata = create_test_metadata();

        adapter.register_skill(&metadata);

        // 检查信任的工具
        assert!(!adapter.needs_confirmation("test-skill", "read_file", &json!({})));

        // 检查需要确认的工具
        assert!(adapter.needs_confirmation("test-skill", "shell_execute", &json!({})));
    }

    #[test]
    fn test_skill_internal_tool_config() {
        let config = SkillInternalToolConfig {
            script_execution_risk: RiskLevel::High,
            resource_access_risk: RiskLevel::Medium,
            require_confirmation_patterns: vec!["dangerous_*".to_string()],
            trusted_script_patterns: vec!["safe_*".to_string()],
        };

        // 信任的脚本
        assert!(!config.needs_confirmation("safe_script.py"));

        // 需要确认的脚本
        assert!(config.needs_confirmation("dangerous_script.sh"));

        // 默认根据风险级别（High >= Medium，需要确认）
        assert!(config.needs_confirmation("unknown_script.py"));
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("*.py", "script.py"));
        assert!(glob_match("test_*", "test_something"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("*.py", "script.sh"));
        assert!(!glob_match("test_*", "other_test"));
    }
}
