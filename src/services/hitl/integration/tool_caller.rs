//! HITL 工具调用集成
//!
//! 提供工具执行前的 HITL 检查和用户确认机制。
//!
//! # 工作流程
//!
//! ```text
//! 1. 接收工具调用请求
//! 2. 评估工具风险 (三层策略)
//! 3. 如果需要确认:
//!    a. 构建操作预览
//!    b. 创建 HITL 请求
//!    c. 等待用户响应
//!    d. 根据响应决定后续操作
//! 4. 执行工具并返回结果
//! ```

use std::{future::Future, sync::Arc};

use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use super::preview::PreviewBuilder;
use crate::services::hitl::{
    manager::HitlManager,
    risk_assessor::RiskAssessment,
    types::{HitlError, HitlResponse, RiskLevel},
};

/// HITL 工具执行结果
#[derive(Debug, Clone)]
pub enum HitlToolResult<T> {
    /// 工具正常执行完成
    Executed(T),
    /// 低风险，直接执行（无需确认）
    ExecutedWithoutConfirmation(T),
    /// 用户批准后执行
    Approved(T),
    /// 用户修改参数后执行
    Modified {
        /// 执行结果
        result: T,
        /// 应用的修改
        modifications: serde_json::Value,
    },
    /// 被用户拒绝
    Rejected {
        /// 拒绝原因
        reason: Option<String>,
    },
    /// 被跳过（超时行为为 Skip）
    Skipped {
        /// 跳过原因
        reason: String,
    },
    /// 被用户中止
    Aborted {
        /// 中止原因
        reason: Option<String>,
    },
    /// 请求超时
    TimedOut {
        /// 超时后的行为
        behavior: String,
    },
    /// HITL 已禁用
    HitlDisabled(T),
}

impl<T> HitlToolResult<T> {
    /// 检查是否成功执行
    pub fn is_executed(&self) -> bool {
        matches!(
            self,
            HitlToolResult::Executed(_)
                | HitlToolResult::ExecutedWithoutConfirmation(_)
                | HitlToolResult::Approved(_)
                | HitlToolResult::Modified { .. }
                | HitlToolResult::HitlDisabled(_)
        )
    }

    /// 获取执行结果（如果有）
    pub fn result(&self) -> Option<&T> {
        match self {
            HitlToolResult::Executed(r)
            | HitlToolResult::ExecutedWithoutConfirmation(r)
            | HitlToolResult::Approved(r)
            | HitlToolResult::HitlDisabled(r) => Some(r),
            HitlToolResult::Modified { result, .. } => Some(result),
            _ => None,
        }
    }

    /// 获取执行结果（如果有），消耗 self
    pub fn into_result(self) -> Option<T> {
        match self {
            HitlToolResult::Executed(r)
            | HitlToolResult::ExecutedWithoutConfirmation(r)
            | HitlToolResult::Approved(r)
            | HitlToolResult::HitlDisabled(r) => Some(r),
            HitlToolResult::Modified { result, .. } => Some(result),
            _ => None,
        }
    }

    /// 转换为错误消息（如果是失败状态）
    pub fn error_message(&self) -> Option<String> {
        match self {
            HitlToolResult::Rejected { reason } => Some(
                reason
                    .clone()
                    .unwrap_or_else(|| "操作被用户拒绝".to_string()),
            ),
            HitlToolResult::Skipped { reason } => Some(reason.clone()),
            HitlToolResult::Aborted { reason } => Some(
                reason
                    .clone()
                    .unwrap_or_else(|| "操作被用户中止".to_string()),
            ),
            HitlToolResult::TimedOut { behavior } => {
                Some(format!("HITL 请求超时，超时行为: {}", behavior))
            }
            _ => None,
        }
    }
}

/// HITL 工具调用上下文
#[derive(Debug, Clone)]
pub struct HitlToolContext {
    /// 会话 ID
    pub conversation_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 子任务 ID（可选）
    pub subtask_id: Option<usize>,
    /// Sub-Agent ID（可选）
    pub subagent_id: Option<String>,
    /// 取消令牌（可选，用于取消等待中的 HITL 请求）
    pub cancel_token: Option<CancellationToken>,
}

impl HitlToolContext {
    /// 创建新的上下文
    pub fn new(conversation_id: impl Into<String>, user_id: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            user_id: user_id.into(),
            subtask_id: None,
            subagent_id: None,
            cancel_token: None,
        }
    }

    /// 设置子任务 ID
    pub fn with_subtask_id(mut self, subtask_id: usize) -> Self {
        self.subtask_id = Some(subtask_id);
        self
    }

    /// 设置 Sub-Agent ID
    pub fn with_subagent_id(mut self, subagent_id: impl Into<String>) -> Self {
        self.subagent_id = Some(subagent_id.into());
        self
    }

    /// 设置取消令牌
    pub fn with_cancel_token(mut self, cancel_token: CancellationToken) -> Self {
        self.cancel_token = Some(cancel_token);
        self
    }
}

/// HITL 工具调用器
///
/// 封装工具执行逻辑，在执行前进行 HITL 检查。
pub struct HitlToolCaller {
    manager: Arc<HitlManager>,
}

impl HitlToolCaller {
    /// 创建新的工具调用器
    pub fn new(manager: Arc<HitlManager>) -> Self {
        Self { manager }
    }

    /// 评估工具风险
    pub fn assess_risk(&self, tool_name: &str, args: &serde_json::Value) -> RiskAssessment {
        self.manager.assess_risk(tool_name, args)
    }

    /// 检查并执行工具
    ///
    /// 根据风险评估结果决定是否需要用户确认。
    ///
    /// # 参数
    /// - `tool_name`: 工具名称
    /// - `args`: 工具参数
    /// - `context`: 调用上下文
    /// - `execute_fn`: 工具执行函数
    ///
    /// # 返回
    /// - `Ok(HitlToolResult<T>)`: 执行结果
    /// - `Err(HitlError)`: HITL 错误
    pub async fn check_and_execute<T, F, Fut>(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        // 1. 评估风险
        let assessment = self.assess_risk(tool_name, args);

        debug!(
            tool_name = %tool_name,
            final_risk = ?assessment.final_risk,
            requires_confirmation = %assessment.requires_confirmation,
            "Tool risk assessed"
        );

        // 2. 如果不需要确认，直接执行
        if !assessment.requires_confirmation {
            info!(
                tool_name = %tool_name,
                risk = ?assessment.final_risk,
                "Executing tool without HITL confirmation"
            );

            return match execute_fn(args.clone()).await {
                Ok(result) => Ok(HitlToolResult::ExecutedWithoutConfirmation(result)),
                Err(e) => Err(HitlError::Internal(format!("Tool execution failed: {}", e))),
            };
        }

        // 3. 需要确认，创建 HITL 请求
        self.execute_with_confirmation(tool_name, args, context, &assessment, execute_fn)
            .await
    }

    /// 带确认的工具执行
    async fn execute_with_confirmation<T, F, Fut>(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        assessment: &RiskAssessment,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        // 构建操作预览
        let preview = PreviewBuilder::build(tool_name, args);

        info!(
            tool_name = %tool_name,
            risk = ?assessment.final_risk,
            conversation_id = %context.conversation_id,
            "Creating HITL confirmation request"
        );

        // 创建确认请求（包含 subtask_id 和 subagent_id）
        let request = self
            .manager
            .create_confirmation_request(
                tool_name,
                args,
                assessment,
                preview,
                &context.conversation_id,
                &context.user_id,
                context.subtask_id,
                context.subagent_id.clone(),
            )
            .await?;

        let request_id = request.id.clone();

        info!(
            request_id = %request_id,
            tool_name = %tool_name,
            "Waiting for user response"
        );

        // 等待用户响应（支持取消）
        let response = self
            .manager
            .wait_for_response_with_cancel(&request_id, context.cancel_token.as_ref())
            .await?;

        // 处理响应
        self.process_response(response, tool_name, args, execute_fn)
            .await
    }

    /// 处理用户响应
    async fn process_response<T, F, Fut>(
        &self,
        response: HitlResponse,
        tool_name: &str,
        original_args: &serde_json::Value,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        match response {
            HitlResponse::Approve => {
                info!(tool_name = %tool_name, "User approved tool execution");

                match execute_fn(original_args.clone()).await {
                    Ok(result) => Ok(HitlToolResult::Approved(result)),
                    Err(e) => Err(HitlError::Internal(format!("Tool execution failed: {}", e))),
                }
            }

            HitlResponse::Modify { modifications } => {
                info!(
                    tool_name = %tool_name,
                    modifications = ?modifications,
                    "User modified tool arguments"
                );

                // 应用修改
                let modified_args = Self::apply_modifications(original_args, &modifications);

                match execute_fn(modified_args).await {
                    Ok(result) => Ok(HitlToolResult::Modified {
                        result,
                        modifications,
                    }),
                    Err(e) => Err(HitlError::Internal(format!(
                        "Modified tool execution failed: {}",
                        e
                    ))),
                }
            }

            HitlResponse::Reject { reason } => {
                warn!(
                    tool_name = %tool_name,
                    reason = ?reason,
                    "User rejected tool execution"
                );
                Ok(HitlToolResult::Rejected { reason })
            }

            HitlResponse::Abort { reason } => {
                warn!(
                    tool_name = %tool_name,
                    reason = ?reason,
                    "User aborted tool execution"
                );
                Ok(HitlToolResult::Aborted { reason })
            }

            HitlResponse::Resume => {
                // Resume 通常用于 Pause 请求，对于确认请求视为批准
                info!(tool_name = %tool_name, "User resumed, treating as approval");

                match execute_fn(original_args.clone()).await {
                    Ok(result) => Ok(HitlToolResult::Approved(result)),
                    Err(e) => Err(HitlError::Internal(format!("Tool execution failed: {}", e))),
                }
            }

            HitlResponse::Clarify { .. } | HitlResponse::ProvideFeedback { .. } => {
                // 这些响应类型不应该用于确认请求
                Err(HitlError::InvalidResponse(
                    "Unexpected response type for confirmation request".to_string(),
                ))
            }
        }
    }

    /// 应用用户修改到参数
    pub fn apply_modifications(
        original: &serde_json::Value,
        modifications: &serde_json::Value,
    ) -> serde_json::Value {
        let mut result = original.clone();

        if let (Some(base), Some(mods)) = (result.as_object_mut(), modifications.as_object()) {
            for (key, value) in mods {
                base.insert(key.clone(), value.clone());
            }
        }

        result
    }

    /// 强制确认执行（忽略风险评估结果）
    ///
    /// 无论风险级别如何，都创建 HITL 请求等待用户确认。
    pub async fn force_confirm_and_execute<T, F, Fut>(
        &self,
        tool_name: &str,
        args: &serde_json::Value,
        context: &HitlToolContext,
        execute_fn: F,
    ) -> Result<HitlToolResult<T>, HitlError>
    where
        F: FnOnce(serde_json::Value) -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        // 强制评估为高风险
        let mut assessment = self.assess_risk(tool_name, args);
        assessment.final_risk = RiskLevel::High;
        assessment.requires_confirmation = true;

        self.execute_with_confirmation(tool_name, args, context, &assessment, execute_fn)
            .await
    }
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

    fn create_test_context() -> HitlToolContext {
        HitlToolContext::new("conv_test", "user_test")
    }

    #[test]
    fn test_apply_modifications() {
        let original = json!({
            "path": "/tmp/test.txt",
            "content": "original content"
        });

        let modifications = json!({
            "content": "modified content",
            "new_field": "new value"
        });

        let result = HitlToolCaller::apply_modifications(&original, &modifications);

        assert_eq!(result["path"], "/tmp/test.txt");
        assert_eq!(result["content"], "modified content");
        assert_eq!(result["new_field"], "new value");
    }

    #[test]
    fn test_hitl_tool_result_is_executed() {
        let result: HitlToolResult<String> = HitlToolResult::Approved("success".to_string());
        assert!(result.is_executed());

        let result: HitlToolResult<String> = HitlToolResult::Rejected { reason: None };
        assert!(!result.is_executed());
    }

    #[test]
    fn test_hitl_tool_result_error_message() {
        let result: HitlToolResult<String> = HitlToolResult::Rejected {
            reason: Some("Not allowed".to_string()),
        };
        assert_eq!(result.error_message(), Some("Not allowed".to_string()));

        let result: HitlToolResult<String> = HitlToolResult::Approved("success".to_string());
        assert_eq!(result.error_message(), None);
    }

    #[test]
    fn test_hitl_tool_context() {
        let context = HitlToolContext::new("conv1", "user1")
            .with_subtask_id(42)
            .with_subagent_id("agent_123");

        assert_eq!(context.conversation_id, "conv1");
        assert_eq!(context.user_id, "user1");
        assert_eq!(context.subtask_id, Some(42));
        assert_eq!(context.subagent_id, Some("agent_123".to_string()));
    }

    #[tokio::test]
    async fn test_check_and_execute_low_risk() {
        let manager = create_test_manager();
        let caller = HitlToolCaller::new(manager);
        let context = create_test_context();

        // 低风险工具应该直接执行
        let result = caller
            .check_and_execute(
                "mcp__read__get_file",
                &json!({"path": "/tmp/test.txt"}),
                &context,
                |_args| async { Ok::<String, String>("file content".to_string()) },
            )
            .await;

        assert!(result.is_ok());
        let result = result.unwrap();
        assert!(matches!(
            result,
            HitlToolResult::ExecutedWithoutConfirmation(_)
        ));
    }
}
