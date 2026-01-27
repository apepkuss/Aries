//! HITL SSE Notifier
//!
//! 将 HitlManager 的内部事件桥接到 SSE EventEmitter，
//! 使前端能够实时接收 HITL 请求和状态变化通知。
//!
//! # 使用方式
//!
//! ```ignore
//! // 创建 notifier
//! let notifier = HitlNotifier::new(hitl_manager.clone(), emitter.clone());
//!
//! // 启动后台监听任务
//! let handle = notifier.start();
//!
//! // 或者通过 spawn 方法自动管理生命周期
//! HitlNotifier::spawn(hitl_manager.clone(), emitter.clone());
//! ```

use std::sync::Arc;

use tokio::task::JoinHandle;
use tracing::{debug, warn};

use super::{
    manager::{HitlEvent, HitlManager},
    types::HitlRequestType,
};
use crate::chat::emitter::EventEmitter;

/// HITL SSE Notifier
///
/// 监听 HitlManager 的事件并通过 EventEmitter 发送到 SSE 流。
pub struct HitlNotifier {
    /// HITL Manager 引用
    manager: Arc<HitlManager>,
    /// Event Emitter 引用
    emitter: Arc<dyn EventEmitter>,
}

impl HitlNotifier {
    /// 创建新的 Notifier
    pub fn new(manager: Arc<HitlManager>, emitter: Arc<dyn EventEmitter>) -> Self {
        Self { manager, emitter }
    }

    /// 启动后台监听任务
    ///
    /// 返回 JoinHandle 用于等待或取消任务
    pub fn start(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            self.run().await;
        })
    }

    /// 创建并启动 notifier（fire and forget）
    pub fn spawn(manager: Arc<HitlManager>, emitter: Arc<dyn EventEmitter>) {
        let notifier = Self::new(manager, emitter);
        tokio::spawn(async move {
            notifier.run().await;
        });
    }

    /// 运行事件监听循环
    async fn run(&self) {
        let mut receiver = self.manager.subscribe();

        debug!("HITL notifier started");

        loop {
            match receiver.recv().await {
                Ok(event) => {
                    self.handle_event(event).await;
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    debug!("HITL event channel closed, notifier stopping");
                    break;
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(count)) => {
                    warn!(count = count, "HITL notifier lagged, some events missed");
                }
            }
        }

        debug!("HITL notifier stopped");
    }

    /// 处理单个事件
    async fn handle_event(&self, event: HitlEvent) {
        match event {
            HitlEvent::Request(request) => {
                // 提取请求类型和相关信息
                let (request_type, risk_level, tool_name, summary) = match &request.request_type {
                    HitlRequestType::Confirmation(conf) => (
                        "confirmation",
                        Some(format!("{:?}", conf.risk_level)),
                        Some(conf.tool_name.clone()),
                        conf.summary.clone(),
                    ),
                    HitlRequestType::Clarification(clar) => {
                        ("clarification", None, None, clar.question.clone())
                    }
                    HitlRequestType::Feedback(fb) => ("feedback", None, None, fb.subject.clone()),
                    HitlRequestType::Pause(pause) => {
                        ("pause", None, None, format!("{:?}", pause.reason))
                    }
                };

                self.emitter
                    .emit_hitl_request(
                        &request.id,
                        request_type,
                        &summary,
                        risk_level.as_deref(),
                        tool_name.as_deref(),
                        &request.conversation_id,
                        &request.user_id,
                        &request.expires_at.to_rfc3339(),
                        request.remaining_seconds(),
                        &format!("{:?}", request.timeout_behavior),
                        request.subtask_id,
                        request.subagent_id.as_deref(),
                    )
                    .await;

                debug!(
                    request_id = %request.id,
                    request_type = request_type,
                    "HITL request event emitted"
                );
            }

            HitlEvent::Status {
                request_id,
                status,
                message,
            } => {
                self.emitter
                    .emit_hitl_status(&request_id, &format!("{:?}", status), &message)
                    .await;

                debug!(
                    request_id = %request_id,
                    status = ?status,
                    "HITL status event emitted"
                );
            }

            HitlEvent::TimeoutWarning {
                request_id,
                remaining_seconds,
            } => {
                self.emitter
                    .emit_hitl_timeout_warning(&request_id, remaining_seconds)
                    .await;

                debug!(
                    request_id = %request_id,
                    remaining_seconds = remaining_seconds,
                    "HITL timeout warning event emitted"
                );
            }
        }
    }
}

/// 为单个请求创建 HITL 事件发射器
///
/// 这个结构体用于在工具执行过程中发射 HITL 相关事件。
pub struct HitlEventBridge {
    emitter: Arc<dyn EventEmitter>,
}

impl HitlEventBridge {
    /// 创建新的事件桥接器
    pub fn new(emitter: Arc<dyn EventEmitter>) -> Self {
        Self { emitter }
    }

    /// 发射 HITL 请求事件
    #[allow(clippy::too_many_arguments)]
    pub async fn emit_request(
        &self,
        request_id: &str,
        request_type: &str,
        summary: &str,
        risk_level: Option<&str>,
        tool_name: Option<&str>,
        conversation_id: &str,
        user_id: &str,
        expires_at: &str,
        remaining_seconds: i64,
        timeout_behavior: &str,
        subtask_id: Option<usize>,
        subagent_id: Option<&str>,
    ) {
        self.emitter
            .emit_hitl_request(
                request_id,
                request_type,
                summary,
                risk_level,
                tool_name,
                conversation_id,
                user_id,
                expires_at,
                remaining_seconds,
                timeout_behavior,
                subtask_id,
                subagent_id,
            )
            .await;
    }

    /// 发射 HITL 状态变化事件
    pub async fn emit_status(&self, request_id: &str, status: &str, message: &str) {
        self.emitter
            .emit_hitl_status(request_id, status, message)
            .await;
    }

    /// 发射 HITL 超时警告事件
    pub async fn emit_timeout_warning(&self, request_id: &str, remaining_seconds: u64) {
        self.emitter
            .emit_hitl_timeout_warning(request_id, remaining_seconds)
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{chat::emitter::NoopEventEmitter, services::hitl::config::HitlConfig};

    #[tokio::test]
    async fn test_hitl_event_bridge() {
        let emitter: Arc<dyn EventEmitter> = Arc::new(NoopEventEmitter);
        let bridge = HitlEventBridge::new(emitter);

        // 测试事件发射不会 panic
        bridge
            .emit_request(
                "req_123",
                "confirmation",
                "Test operation",
                Some("high"),
                Some("test_tool"),
                "conv_1",
                "user_1",
                "2024-01-01T00:00:00Z",
                300,
                "Reject",
                None,
                None,
            )
            .await;

        bridge
            .emit_status("req_123", "approved", "User approved")
            .await;

        bridge.emit_timeout_warning("req_123", 30).await;
    }

    #[tokio::test]
    async fn test_hitl_notifier_creation() {
        let config = HitlConfig::default();
        let manager = Arc::new(HitlManager::new(config, 100));
        let emitter: Arc<dyn EventEmitter> = Arc::new(NoopEventEmitter);

        // 测试创建不会 panic
        let _notifier = HitlNotifier::new(manager, emitter);
    }
}
