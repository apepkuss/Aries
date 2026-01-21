//! Sub-Agent 反思器
//!
//! 此模块提供 Sub-Agent 执行过程中的反思功能，
//! 用于自我评估和纠错。

use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::debug;

use super::config::SubAgentReflectionConfig;
use crate::{
    error::{ServerError, ServerResult},
    reflection::{
        RecommendedAction, ReflectionConfig, ReflectionContext, ReflectionEngine, ReflectionResult,
        engine::LlmServerInfo,
    },
};

// ============================================================================
// SubAgentReflector
// ============================================================================

/// Sub-Agent 反思器
///
/// 封装 Sub-Agent 执行过程中的反思逻辑
pub struct SubAgentReflector {
    /// 反思配置
    config: SubAgentReflectionConfig,
    /// 反思引擎
    engine: Option<ReflectionEngine>,
    /// 当前重试计数
    retry_count: u32,
    /// 最后的反思结果
    last_reflection: Option<ReflectionResult>,
}

impl SubAgentReflector {
    /// 创建新的反思器
    pub fn new(
        config: SubAgentReflectionConfig,
        llm_server: Option<Arc<RwLock<LlmServerInfo>>>,
    ) -> Self {
        let engine = if config.enabled {
            llm_server.map(|server| {
                ReflectionEngine::new(
                    server,
                    ReflectionConfig {
                        enabled: true,
                        confidence_threshold: config.confidence_threshold,
                        max_reflection_rounds: config.max_retries,
                        enable_result_validation: false, // Sub-Agent 不使用结果验证
                        enable_path_optimization: false,
                    },
                )
            })
        } else {
            None
        };

        Self {
            config,
            engine,
            retry_count: 0,
            last_reflection: None,
        }
    }

    /// 检查是否启用反思
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.engine.is_some()
    }

    /// 检查是否应在指定迭代触发反思
    pub fn should_reflect_at_iteration(&self, iteration: u32) -> bool {
        self.is_enabled() && self.config.should_reflect_at_iteration(iteration)
    }

    /// 检查是否应在工具错误后反思
    pub fn should_reflect_on_tool_error(&self) -> bool {
        self.is_enabled() && self.config.reflect_on_tool_error
    }

    /// 检查是否应在完成时反思
    pub fn should_reflect_on_completion(&self, total_iterations: u32) -> bool {
        self.is_enabled() && self.config.should_reflect_on_completion(total_iterations)
    }

    /// 获取当前重试次数
    pub fn retry_count(&self) -> u32 {
        self.retry_count
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        self.retry_count < self.config.max_retries
    }

    /// 增加重试计数
    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    /// 重置重试计数
    pub fn reset_retry_count(&mut self) {
        self.retry_count = 0;
    }

    /// 获取最后的反思结果
    pub fn last_reflection(&self) -> Option<&ReflectionResult> {
        self.last_reflection.as_ref()
    }

    /// 执行周期性反思
    ///
    /// 在迭代过程中定期检查执行进度
    pub async fn reflect_on_iteration(
        &mut self,
        iteration: u32,
        task_description: &str,
        current_progress: &str,
        tool_history: &[String],
    ) -> ServerResult<ReflectionAction> {
        if !self.should_reflect_at_iteration(iteration) {
            return Ok(ReflectionAction::Continue);
        }

        debug!(
            "Triggering periodic reflection at iteration {} for task: {}",
            iteration,
            truncate_str(task_description, 50)
        );

        let context = ReflectionContext::new(task_description)
            .with_iterations(iteration)
            .with_tool_calls(tool_history.to_vec());

        self.perform_reflection(current_progress, &context).await
    }

    /// 执行工具错误后的反思
    pub async fn reflect_on_tool_error(
        &mut self,
        task_description: &str,
        tool_name: &str,
        error_message: &str,
        tool_history: &[String],
    ) -> ServerResult<ReflectionAction> {
        if !self.should_reflect_on_tool_error() {
            return Ok(ReflectionAction::Continue);
        }

        debug!(
            "Triggering reflection after tool error: {} - {}",
            tool_name, error_message
        );

        let progress = format!(
            "Tool call '{}' failed with error: {}\n\nAttempting to recover and continue.",
            tool_name, error_message
        );

        let context = ReflectionContext::new(task_description)
            .with_tool_calls(tool_history.to_vec())
            .with_errors(vec![format!("{}: {}", tool_name, error_message)]);

        self.perform_reflection(&progress, &context).await
    }

    /// 执行完成时的反思
    pub async fn reflect_on_completion(
        &mut self,
        total_iterations: u32,
        task_description: &str,
        final_answer: &str,
        tool_history: &[String],
    ) -> ServerResult<ReflectionAction> {
        if !self.should_reflect_on_completion(total_iterations) {
            return Ok(ReflectionAction::Accept);
        }

        debug!(
            "Triggering completion reflection after {} iterations",
            total_iterations
        );

        let context = ReflectionContext::new(task_description)
            .with_iterations(total_iterations)
            .with_tool_calls(tool_history.to_vec());

        self.perform_reflection(final_answer, &context).await
    }

    /// 执行反思并返回动作
    async fn perform_reflection(
        &mut self,
        result: &str,
        context: &ReflectionContext,
    ) -> ServerResult<ReflectionAction> {
        let engine = self.engine.as_ref().ok_or_else(|| {
            ServerError::Operation("Reflection engine not initialized".to_string())
        })?;

        // 创建一个用于反思的 SubTask
        let subtask = crate::chat::planner::SubTask::new(0, context.task_description.clone());

        // 创建 SubtaskTrace
        let trace = crate::chat::trace::SubtaskTrace::new(0, context.task_description.clone());

        // 调用反思引擎
        let reflection = engine
            .reflect_on_subtask(&subtask, result, &trace, context)
            .await?;

        debug!(
            "Reflection result: passed={}, confidence={:.2}, action={:?}",
            reflection.passed, reflection.confidence, reflection.recommended_action
        );

        // 保存反思结果
        self.last_reflection = Some(reflection.clone());

        // 转换为 ReflectionAction
        Ok(self.convert_to_action(&reflection))
    }

    /// 将 ReflectionResult 转换为 ReflectionAction
    fn convert_to_action(&mut self, reflection: &ReflectionResult) -> ReflectionAction {
        if reflection.passed && reflection.confidence >= self.config.confidence_threshold {
            return ReflectionAction::Accept;
        }

        match &reflection.recommended_action {
            RecommendedAction::Accept => ReflectionAction::Accept,

            RecommendedAction::AcceptWithFix(fix) => {
                ReflectionAction::AcceptWithGuidance(fix.clone())
            }

            RecommendedAction::Retry => {
                if self.can_retry() {
                    self.increment_retry();
                    ReflectionAction::Retry {
                        reason: "Reflection suggested retry".to_string(),
                    }
                } else {
                    ReflectionAction::Accept // 超过重试限制，接受当前结果
                }
            }

            RecommendedAction::RetryWithStrategy(strategy) => {
                if self.can_retry() {
                    self.increment_retry();
                    ReflectionAction::RetryWithGuidance {
                        guidance: strategy.clone(),
                    }
                } else {
                    ReflectionAction::AcceptWithGuidance(format!("Consider: {}", strategy))
                }
            }

            RecommendedAction::Replan(request) => ReflectionAction::Abort {
                reason: format!("Replanning required: {}", request.reason),
            },

            RecommendedAction::RequestClarification(question) => {
                // Sub-Agent 无法请求用户澄清，记录并继续
                debug!(
                    "Clarification requested but not available in sub-agent: {}",
                    question
                );
                ReflectionAction::Continue
            }

            RecommendedAction::Abort(reason) => ReflectionAction::Abort {
                reason: reason.clone(),
            },
        }
    }
}

// ============================================================================
// ReflectionAction
// ============================================================================

/// 反思后的动作建议
#[derive(Debug, Clone)]
pub enum ReflectionAction {
    /// 接受当前结果，继续执行
    Accept,

    /// 接受但附带建议
    AcceptWithGuidance(String),

    /// 继续执行（不采取特殊动作）
    Continue,

    /// 重试当前操作
    Retry {
        /// 重试原因
        reason: String,
    },

    /// 使用指导重试
    RetryWithGuidance {
        /// 改进建议
        guidance: String,
    },

    /// 中止执行
    Abort {
        /// 中止原因
        reason: String,
    },
}

impl ReflectionAction {
    /// 检查是否应该继续执行
    pub fn should_continue(&self) -> bool {
        matches!(
            self,
            ReflectionAction::Accept
                | ReflectionAction::AcceptWithGuidance(_)
                | ReflectionAction::Continue
        )
    }

    /// 检查是否需要重试
    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            ReflectionAction::Retry { .. } | ReflectionAction::RetryWithGuidance { .. }
        )
    }

    /// 检查是否应该中止
    pub fn should_abort(&self) -> bool {
        matches!(self, ReflectionAction::Abort { .. })
    }

    /// 获取指导内容（如果有）
    pub fn guidance(&self) -> Option<&str> {
        match self {
            ReflectionAction::AcceptWithGuidance(g) => Some(g),
            ReflectionAction::RetryWithGuidance { guidance } => Some(guidance),
            _ => None,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 截断字符串
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reflector_disabled() {
        let config = SubAgentReflectionConfig::disabled();
        let reflector = SubAgentReflector::new(config, None);

        assert!(!reflector.is_enabled());
        assert!(!reflector.should_reflect_at_iteration(5));
        assert!(!reflector.should_reflect_on_tool_error());
        assert!(!reflector.should_reflect_on_completion(10));
    }

    #[test]
    fn test_reflector_enabled_no_engine() {
        let config = SubAgentReflectionConfig::enabled();
        let reflector = SubAgentReflector::new(config, None);

        // 启用配置但没有引擎，仍然应该返回 false
        assert!(!reflector.is_enabled());
    }

    #[test]
    fn test_reflection_action_properties() {
        let accept = ReflectionAction::Accept;
        assert!(accept.should_continue());
        assert!(!accept.should_retry());
        assert!(!accept.should_abort());

        let retry = ReflectionAction::Retry {
            reason: "test".to_string(),
        };
        assert!(!retry.should_continue());
        assert!(retry.should_retry());
        assert!(!retry.should_abort());

        let abort = ReflectionAction::Abort {
            reason: "test".to_string(),
        };
        assert!(!abort.should_continue());
        assert!(!abort.should_retry());
        assert!(abort.should_abort());
    }

    #[test]
    fn test_reflection_action_guidance() {
        let with_guidance = ReflectionAction::AcceptWithGuidance("improve this".to_string());
        assert_eq!(with_guidance.guidance(), Some("improve this"));

        let retry_guidance = ReflectionAction::RetryWithGuidance {
            guidance: "try differently".to_string(),
        };
        assert_eq!(retry_guidance.guidance(), Some("try differently"));

        let accept = ReflectionAction::Accept;
        assert_eq!(accept.guidance(), None);
    }

    #[test]
    fn test_retry_count() {
        let config = SubAgentReflectionConfig::enabled().with_interval(5);
        let mut reflector = SubAgentReflector::new(config, None);

        assert_eq!(reflector.retry_count(), 0);
        assert!(reflector.can_retry());

        reflector.increment_retry();
        assert_eq!(reflector.retry_count(), 1);

        reflector.increment_retry();
        assert_eq!(reflector.retry_count(), 2);
        assert!(!reflector.can_retry()); // max_retries 默认为 2

        reflector.reset_retry_count();
        assert_eq!(reflector.retry_count(), 0);
        assert!(reflector.can_retry());
    }

    #[test]
    fn test_truncate_str() {
        assert_eq!(truncate_str("short", 10), "short");
        assert_eq!(truncate_str("this is a long string", 10), "this is a ...");
    }
}
