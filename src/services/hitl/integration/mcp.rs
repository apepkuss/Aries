//! MCP (Model Context Protocol) HITL 集成
//!
//! 提供 MCP 工具调用与 HITL 机制的集成支持。
//!
//! # 功能
//!
//! - MCP 工具的风险评估
//! - MCP 服务器配置中的 HITL 规则加载
//! - MCP 工具调用的 HITL 包装
//!
//! # 使用示例
//!
//! ```ignore
//! use crate::services::hitl::integration::mcp::McpHitlAdapter;
//!
//! let adapter = McpHitlAdapter::new(hitl_manager.clone());
//!
//! // 检查 MCP 工具是否需要确认
//! let needs_confirm = adapter.needs_confirmation("filesystem", "write_file", &args);
//!
//! // 使用 HITL 包装执行 MCP 工具
//! let result = adapter.execute_with_hitl(
//!     "filesystem",
//!     "write_file",
//!     &args,
//!     &context,
//!     execute_fn,
//! ).await?;
//! ```

use std::sync::Arc;

use super::tool_caller::{HitlToolCaller, HitlToolContext, HitlToolResult};
use crate::{
    mcp::{format_mcp_tool_name, parse_mcp_tool_name},
    services::hitl::{
        manager::HitlManager,
        risk_assessor::RiskAssessment,
        types::{HitlError, RiskLevel},
    },
};

/// MCP HITL 适配器
///
/// 为 MCP 工具调用提供 HITL 集成支持。
pub struct McpHitlAdapter {
    caller: HitlToolCaller,
    manager: Arc<HitlManager>,
}

impl McpHitlAdapter {
    /// 创建新的 MCP HITL 适配器
    pub fn new(manager: Arc<HitlManager>) -> Self {
        let caller = HitlToolCaller::new(manager.clone());
        Self { caller, manager }
    }

    /// 获取内部的 HitlToolCaller
    pub fn caller(&self) -> &HitlToolCaller {
        &self.caller
    }

    /// 评估 MCP 工具的风险
    ///
    /// # 参数
    /// - `server_name`: MCP 服务器名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    ///
    /// # 返回
    /// 风险评估结果
    pub fn assess_risk(
        &self,
        server_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> RiskAssessment {
        let full_name = format_mcp_tool_name(server_name, tool_name);
        self.manager.assess_risk(&full_name, args)
    }

    /// 检查 MCP 工具是否需要 HITL 确认
    ///
    /// # 参数
    /// - `server_name`: MCP 服务器名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    ///
    /// # 返回
    /// 如果需要确认返回 true
    pub fn needs_confirmation(
        &self,
        server_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> bool {
        self.assess_risk(server_name, tool_name, args)
            .requires_confirmation
    }

    /// 从完整 MCP 工具名称检查是否需要确认
    ///
    /// # 参数
    /// - `full_tool_name`: 完整工具名称（格式：mcp__server__tool）
    /// - `args`: 工具参数
    ///
    /// # 返回
    /// 如果需要确认返回 true，如果不是 MCP 工具也返回 false
    pub fn needs_confirmation_full_name(
        &self,
        full_tool_name: &str,
        args: &serde_json::Value,
    ) -> bool {
        if let Some((server_name, tool_name)) = parse_mcp_tool_name(full_tool_name) {
            self.needs_confirmation(server_name, tool_name, args)
        } else {
            false
        }
    }

    /// 使用 HITL 包装执行 MCP 工具
    ///
    /// # 参数
    /// - `server_name`: MCP 服务器名称
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    /// - `context`: HITL 上下文
    /// - `execute_fn`: 工具执行函数
    ///
    /// # 返回
    /// 执行结果
    pub async fn execute_with_hitl<T, F, Fut>(
        &self,
        server_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        let full_name = format_mcp_tool_name(server_name, tool_name);
        self.caller
            .check_and_execute(&full_name, args, context, execute_fn)
            .await
    }

    /// 从完整工具名称执行 MCP 工具（带 HITL）
    ///
    /// # 参数
    /// - `full_tool_name`: 完整工具名称（格式：mcp__server__tool）
    /// - `args`: 工具参数
    /// - `context`: HITL 上下文
    /// - `execute_fn`: 工具执行函数
    ///
    /// # 返回
    /// 执行结果，如果不是 MCP 工具格式则返回错误
    pub async fn execute_with_hitl_full_name<T, F, Fut>(
        &self,
        full_tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: std::future::Future<Output = Result<T, String>>,
    {
        if let Some((server_name, tool_name)) = parse_mcp_tool_name(full_tool_name) {
            self.execute_with_hitl(server_name, tool_name, args, context, execute_fn)
                .await
        } else {
            Err(HitlError::InvalidResponse(format!(
                "Invalid MCP tool name format: {}",
                full_tool_name
            )))
        }
    }

    /// 获取 MCP 工具的风险级别
    pub fn get_risk_level(
        &self,
        server_name: &str,
        tool_name: &str,
        args: &serde_json::Value,
    ) -> RiskLevel {
        self.assess_risk(server_name, tool_name, args).final_risk
    }
}

/// MCP 工具风险分类
///
/// 用于快速分类 MCP 工具的风险级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpToolCategory {
    /// 只读操作（查询、搜索、获取信息）
    ReadOnly,
    /// 写入操作（创建、修改文件）
    Write,
    /// 删除操作
    Delete,
    /// Shell/命令执行
    Execute,
    /// 网络操作（HTTP 请求、API 调用）
    Network,
    /// 其他/未分类
    Other,
}

impl McpToolCategory {
    /// 从工具名称推断类别
    pub fn from_tool_name(tool_name: &str) -> Self {
        let lower = tool_name.to_lowercase();

        // 删除操作
        if lower.contains("delete") || lower.contains("remove") || lower.contains("drop") {
            return Self::Delete;
        }

        // 执行操作
        if lower.contains("exec")
            || lower.contains("shell")
            || lower.contains("bash")
            || lower.contains("run")
            || lower.contains("command")
        {
            return Self::Execute;
        }

        // 网络操作
        if lower.contains("http")
            || lower.contains("fetch")
            || lower.contains("request")
            || lower.contains("api")
            || lower.contains("curl")
        {
            return Self::Network;
        }

        // 写入操作
        if lower.contains("write")
            || lower.contains("create")
            || lower.contains("update")
            || lower.contains("modify")
            || lower.contains("edit")
            || lower.contains("set")
            || lower.contains("put")
            || lower.contains("post")
            || lower.contains("insert")
        {
            return Self::Write;
        }

        // 只读操作
        if lower.contains("read")
            || lower.contains("get")
            || lower.contains("list")
            || lower.contains("search")
            || lower.contains("query")
            || lower.contains("find")
            || lower.contains("show")
            || lower.contains("describe")
        {
            return Self::ReadOnly;
        }

        Self::Other
    }

    /// 获取默认风险级别
    pub fn default_risk_level(&self) -> RiskLevel {
        match self {
            Self::ReadOnly => RiskLevel::Low,
            Self::Write => RiskLevel::Medium,
            Self::Delete => RiskLevel::High,
            Self::Execute => RiskLevel::High,
            Self::Network => RiskLevel::Medium,
            Self::Other => RiskLevel::Medium,
        }
    }
}

/// MCP 服务器配置中的 HITL 设置
#[derive(Debug, Clone, Default)]
pub struct McpServerHitlConfig {
    /// 是否启用 HITL
    pub enabled: bool,
    /// 默认风险级别
    pub default_risk_level: Option<RiskLevel>,
    /// 工具级别的风险覆盖
    pub tool_overrides: std::collections::HashMap<String, RiskLevel>,
    /// 受信任的操作模式（跳过确认）
    pub trusted_patterns: Vec<String>,
}

impl McpServerHitlConfig {
    /// 获取工具的风险级别
    pub fn get_tool_risk_level(&self, tool_name: &str) -> Option<RiskLevel> {
        // 首先检查工具级别覆盖
        if let Some(level) = self.tool_overrides.get(tool_name) {
            return Some(*level);
        }

        // 然后检查模式匹配
        for pattern in &self.trusted_patterns {
            if tool_name.contains(pattern) || glob_match(pattern, tool_name) {
                return Some(RiskLevel::Low);
            }
        }

        // 返回默认值
        self.default_risk_level
    }
}

/// 简单的 glob 模式匹配
fn glob_match(pattern: &str, text: &str) -> bool {
    // 支持简单的 * 通配符
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

    #[test]
    fn test_mcp_tool_category_from_name() {
        assert_eq!(
            McpToolCategory::from_tool_name("read_file"),
            McpToolCategory::ReadOnly
        );
        assert_eq!(
            McpToolCategory::from_tool_name("get_content"),
            McpToolCategory::ReadOnly
        );
        assert_eq!(
            McpToolCategory::from_tool_name("write_file"),
            McpToolCategory::Write
        );
        assert_eq!(
            McpToolCategory::from_tool_name("create_document"),
            McpToolCategory::Write
        );
        assert_eq!(
            McpToolCategory::from_tool_name("delete_file"),
            McpToolCategory::Delete
        );
        assert_eq!(
            McpToolCategory::from_tool_name("execute_command"),
            McpToolCategory::Execute
        );
        assert_eq!(
            McpToolCategory::from_tool_name("shell_run"),
            McpToolCategory::Execute
        );
        assert_eq!(
            McpToolCategory::from_tool_name("http_request"),
            McpToolCategory::Network
        );
        assert_eq!(
            McpToolCategory::from_tool_name("some_tool"),
            McpToolCategory::Other
        );
    }

    #[test]
    fn test_mcp_tool_category_default_risk() {
        assert_eq!(
            McpToolCategory::ReadOnly.default_risk_level(),
            RiskLevel::Low
        );
        assert_eq!(
            McpToolCategory::Write.default_risk_level(),
            RiskLevel::Medium
        );
        assert_eq!(
            McpToolCategory::Delete.default_risk_level(),
            RiskLevel::High
        );
        assert_eq!(
            McpToolCategory::Execute.default_risk_level(),
            RiskLevel::High
        );
    }

    #[test]
    fn test_mcp_hitl_adapter_assess_risk() {
        let manager = create_test_manager();
        let adapter = McpHitlAdapter::new(manager);

        // 测试评估结果
        let assessment =
            adapter.assess_risk("filesystem", "write_file", &json!({"path": "/tmp/test"}));
        assert!(assessment.final_risk >= RiskLevel::Low);
    }

    #[test]
    fn test_mcp_hitl_adapter_needs_confirmation() {
        let manager = create_test_manager();
        let adapter = McpHitlAdapter::new(manager);

        // 高风险工具应该需要确认
        let needs = adapter.needs_confirmation("shell", "execute", &json!({"command": "rm -rf /"}));
        assert!(needs);
    }

    #[test]
    fn test_mcp_hitl_adapter_needs_confirmation_full_name() {
        let manager = create_test_manager();
        let adapter = McpHitlAdapter::new(manager);

        // 测试完整名称格式
        let needs = adapter
            .needs_confirmation_full_name("mcp__shell__execute", &json!({"command": "rm -rf /"}));
        assert!(needs);

        // 无效格式
        let needs = adapter.needs_confirmation_full_name("invalid_name", &json!({}));
        assert!(!needs);
    }

    #[test]
    fn test_mcp_server_hitl_config() {
        let mut config = McpServerHitlConfig {
            enabled: true,
            default_risk_level: Some(RiskLevel::Medium),
            tool_overrides: std::collections::HashMap::new(),
            trusted_patterns: vec!["read_*".to_string()],
        };

        config
            .tool_overrides
            .insert("delete_file".to_string(), RiskLevel::Critical);

        // 工具覆盖
        assert_eq!(
            config.get_tool_risk_level("delete_file"),
            Some(RiskLevel::Critical)
        );

        // 模式匹配
        assert_eq!(
            config.get_tool_risk_level("read_file"),
            Some(RiskLevel::Low)
        );

        // 默认值
        assert_eq!(
            config.get_tool_risk_level("some_tool"),
            Some(RiskLevel::Medium)
        );
    }

    #[test]
    fn test_glob_match() {
        assert!(glob_match("read_*", "read_file"));
        assert!(glob_match("*_file", "write_file"));
        assert!(glob_match("get_*_info", "get_user_info"));
        assert!(!glob_match("read_*", "write_file"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "not_exact"));
    }
}
