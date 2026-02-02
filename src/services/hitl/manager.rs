//! HITL Manager - 核心协调器
//!
//! 管理 HITL 请求的生命周期，包括：
//! - 创建请求
//! - 处理用户响应
//! - 超时处理
//! - 状态通知

use std::{collections::HashMap, sync::Arc};

use tokio::{
    select,
    sync::{RwLock, broadcast, oneshot},
    task::JoinHandle,
    time::{Duration, interval},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::{
    config::HitlConfig,
    risk_assessor::{RiskAssessment, RiskAssessor},
    store::PendingStore,
    trust_store::TrustStore,
    types::{
        ConfirmationRequest, HitlError, HitlRequest, HitlRequestId, HitlRequestStatus,
        HitlRequestType, HitlResponse, OperationPreview, TimeoutBehavior,
    },
};

/// HITL 事件（用于 SSE 通知）
#[derive(Debug, Clone)]
pub enum HitlEvent {
    /// 新的 HITL 请求
    Request(Box<HitlRequest>),
    /// 状态更新
    Status {
        request_id: String,
        status: HitlRequestStatus,
        message: String,
    },
    /// 超时警告
    TimeoutWarning {
        request_id: String,
        remaining_seconds: u64,
    },
}

/// 等待响应的通道
type ResponseWaiter = oneshot::Sender<Result<HitlResponse, HitlError>>;

/// HITL Manager - 核心协调器
pub struct HitlManager {
    /// 配置
    config: HitlConfig,
    /// Pending Store
    store: Arc<PendingStore>,
    /// 风险评估器
    risk_assessor: Arc<RiskAssessor>,
    /// 事件广播通道
    event_sender: broadcast::Sender<HitlEvent>,
    /// 等待响应的请求
    waiters: RwLock<HashMap<HitlRequestId, ResponseWaiter>>,
    /// 超时检查间隔（秒）
    timeout_check_interval_secs: u64,
    /// 超时警告提前秒数
    timeout_warning_before_secs: u64,
}

impl HitlManager {
    /// 创建新的 HITL Manager
    pub fn new(config: HitlConfig, max_pending: usize) -> Self {
        let trust_store = Arc::new(TrustStore::new());
        let risk_assessor = Arc::new(RiskAssessor::new(config.clone(), trust_store));
        let store = Arc::new(PendingStore::new(max_pending));
        let (event_sender, _) = broadcast::channel(100);

        Self {
            config,
            store,
            risk_assessor,
            event_sender,
            waiters: RwLock::new(HashMap::new()),
            timeout_check_interval_secs: 1,
            timeout_warning_before_secs: 30,
        }
    }

    /// 使用自定义组件创建 HITL Manager
    pub fn with_components(
        config: HitlConfig,
        store: Arc<PendingStore>,
        risk_assessor: Arc<RiskAssessor>,
    ) -> Self {
        let (event_sender, _) = broadcast::channel(100);

        Self {
            config,
            store,
            risk_assessor,
            event_sender,
            waiters: RwLock::new(HashMap::new()),
            timeout_check_interval_secs: 1,
            timeout_warning_before_secs: 30,
        }
    }

    /// 订阅事件
    pub fn subscribe(&self) -> broadcast::Receiver<HitlEvent> {
        self.event_sender.subscribe()
    }

    /// 检查 HITL 是否启用
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// 评估工具调用风险
    pub fn assess_risk(&self, tool_name: &str, args: &serde_json::Value) -> RiskAssessment {
        self.risk_assessor.assess(tool_name, args)
    }

    /// 创建确认请求
    ///
    /// 用于工具调用前的确认
    #[allow(clippy::too_many_arguments)]
    pub async fn create_confirmation_request(
        &self,
        tool_name: &str,
        tool_args: &serde_json::Value,
        assessment: &RiskAssessment,
        preview: OperationPreview,
        conversation_id: &str,
        user_id: &str,
        subtask_id: Option<usize>,
        subagent_id: Option<String>,
    ) -> Result<HitlRequest, HitlError> {
        let summary = format!("执行 {} 操作", tool_name);

        let confirmation = ConfirmationRequest {
            summary,
            risk_level: assessment.final_risk,
            tool_name: tool_name.to_string(),
            tool_args: tool_args.clone(),
            preview,
            risk_factors: assessment.risk_factors.clone(),
            allow_modification: true,
            modifiable_fields: vec![], // 可以根据工具类型设置
        };

        let timeout_secs = assessment.get_timeout_secs(&self.config);
        let timeout_behavior = assessment.get_timeout_behavior(&self.config);

        self.create_request(
            HitlRequestType::Confirmation(Box::new(confirmation)),
            conversation_id,
            user_id,
            timeout_secs,
            timeout_behavior,
            HashMap::new(),
            subtask_id,
            subagent_id,
        )
        .await
    }

    /// 创建隐私模式确认请求
    ///
    /// 当检测到用户查询包含隐私内容时，创建此请求让用户确认是否使用隐私模式。
    #[allow(clippy::too_many_arguments)]
    pub async fn create_privacy_confirmation_request(
        &self,
        query_summary: &str,
        detected_patterns: Vec<super::types::DetectedPrivacyPattern>,
        detection_method: &str,
        confidence: f32,
        recommendation: &str,
        conversation_id: &str,
        user_id: &str,
    ) -> Result<HitlRequest, HitlError> {
        let privacy_request = super::types::PrivacyModeConfirmationRequest {
            query_summary: query_summary.to_string(),
            detected_patterns,
            detection_method: detection_method.to_string(),
            confidence,
            recommendation: recommendation.to_string(),
        };

        // 使用配置的默认超时时间和行为
        let timeout_secs = self.config.default_timeout_secs;
        let timeout_behavior = self.config.default_timeout_behavior;

        self.create_request(
            HitlRequestType::PrivacyModeConfirmation(privacy_request),
            conversation_id,
            user_id,
            timeout_secs,
            timeout_behavior,
            std::collections::HashMap::new(),
            None,
            None,
        )
        .await
    }

    /// 创建 HITL 请求
    #[allow(clippy::too_many_arguments)]
    pub async fn create_request(
        &self,
        request_type: HitlRequestType,
        conversation_id: &str,
        user_id: &str,
        timeout_secs: u64,
        timeout_behavior: TimeoutBehavior,
        metadata: HashMap<String, serde_json::Value>,
        subtask_id: Option<usize>,
        subagent_id: Option<String>,
    ) -> Result<HitlRequest, HitlError> {
        if !self.config.enabled {
            return Err(HitlError::ConfigError("HITL is disabled".to_string()));
        }

        // 生成唯一 ID
        let id = format!("hitl_{}", Uuid::new_v4().to_string().replace("-", ""));

        // 创建请求
        let mut request = HitlRequest::new(
            id,
            request_type,
            conversation_id.to_string(),
            user_id.to_string(),
            timeout_secs,
            timeout_behavior,
        );
        request.metadata = metadata;
        request.subtask_id = subtask_id;
        request.subagent_id = subagent_id;

        // 存储请求
        self.store
            .insert(request.clone())
            .map_err(|e| HitlError::StoreError(e.to_string()))?;

        // 发送事件
        self.notify_request(&request);

        info!(
            request_id = %request.id,
            conversation_id = %conversation_id,
            subtask_id = ?subtask_id,
            "HITL request created"
        );

        Ok(request)
    }

    /// 等待用户响应
    ///
    /// 阻塞直到用户响应或超时
    pub async fn wait_for_response(&self, request_id: &str) -> Result<HitlResponse, HitlError> {
        self.wait_for_response_with_cancel(request_id, None).await
    }

    /// 等待用户响应（支持取消）
    ///
    /// 阻塞直到用户响应、超时或被取消
    pub async fn wait_for_response_with_cancel(
        &self,
        request_id: &str,
        cancel_token: Option<&CancellationToken>,
    ) -> Result<HitlResponse, HitlError> {
        let request = self
            .store
            .get(request_id)
            .ok_or_else(|| HitlError::RequestNotFound(request_id.to_string()))?;

        // 如果已经有响应，直接返回
        if let Some(response) = request.response {
            return Ok(response);
        }

        // 如果不是待处理状态，返回错误
        if request.status != HitlRequestStatus::Pending {
            return Err(HitlError::NotPending(request_id.to_string()));
        }

        // 创建等待通道
        let (tx, rx) = oneshot::channel();

        // 注册等待者
        {
            let mut waiters = self.waiters.write().await;
            waiters.insert(request_id.to_string(), tx);
        }

        // 等待响应或取消
        if let Some(token) = cancel_token {
            select! {
                result = rx => {
                    match result {
                        Ok(r) => r,
                        Err(_) => Err(HitlError::Internal("Waiter channel closed".to_string())),
                    }
                }
                _ = token.cancelled() => {
                    // 取消时，移除等待者
                    let mut waiters = self.waiters.write().await;
                    waiters.remove(request_id);

                    // 更新请求状态为已取消
                    let _ = self.store.update_status(request_id, HitlRequestStatus::Cancelled);

                    // 发送 SSE 事件通知前端
                    self.notify_status(request_id, HitlRequestStatus::Cancelled, "请求被其他子任务拒绝而取消");

                    info!(
                        request_id = %request_id,
                        "HITL request cancelled by cancellation token"
                    );

                    Err(HitlError::Cancelled(request_id.to_string()))
                }
            }
        } else {
            // 没有取消令牌，直接等待
            match rx.await {
                Ok(result) => result,
                Err(_) => Err(HitlError::Internal("Waiter channel closed".to_string())),
            }
        }
    }

    /// 处理用户响应
    pub async fn handle_response(
        &self,
        request_id: &str,
        user_id: &str,
        response: HitlResponse,
    ) -> Result<HitlRequest, HitlError> {
        // 获取并验证请求
        let request = self
            .store
            .get(request_id)
            .ok_or_else(|| HitlError::RequestNotFound(request_id.to_string()))?;

        // 验证用户权限
        if request.user_id != user_id {
            return Err(HitlError::PermissionDenied(format!(
                "User {} is not authorized to respond to request {}",
                user_id, request_id
            )));
        }

        // 验证请求状态
        if !request.status.can_respond() {
            return Err(HitlError::NotPending(request_id.to_string()));
        }

        // 验证请求是否过期
        if request.is_expired() {
            return Err(HitlError::RequestExpired(request_id.to_string()));
        }

        // 验证响应类型是否匹配
        self.validate_response(&request.request_type, &response)?;

        // 更新请求状态
        let new_status = self.response_to_status(&response);
        let updated_request = self.store.update(request_id, |r| {
            r.status = new_status;
            r.response = Some(response.clone());
        })?;

        // 记录批准（用于 L3 信任积累）
        if matches!(
            response,
            HitlResponse::Approve | HitlResponse::Modify { .. }
        ) && let HitlRequestType::Confirmation(ref conf) = request.request_type
        {
            self.risk_assessor
                .record_approval(&conf.tool_name, &conf.tool_args);
        }

        // 通知等待者
        self.notify_waiter(request_id, Ok(response.clone())).await;

        // 发送状态更新事件
        self.notify_status(request_id, new_status, "用户已响应");

        info!(
            request_id = %request_id,
            status = ?new_status,
            "HITL response handled"
        );

        Ok(updated_request)
    }

    /// 取消请求
    pub async fn cancel_request(&self, request_id: &str, user_id: &str) -> Result<(), HitlError> {
        // 获取并验证请求
        let request = self
            .store
            .get(request_id)
            .ok_or_else(|| HitlError::RequestNotFound(request_id.to_string()))?;

        // 验证用户权限
        if request.user_id != user_id {
            return Err(HitlError::PermissionDenied(format!(
                "User {} is not authorized to cancel request {}",
                user_id, request_id
            )));
        }

        // 验证请求状态
        if request.status.is_terminal() {
            return Err(HitlError::NotPending(request_id.to_string()));
        }

        // 更新状态
        self.store
            .update_status(request_id, HitlRequestStatus::Cancelled)?;

        // 通知等待者
        self.notify_waiter(
            request_id,
            Err(HitlError::RequestNotFound("Request cancelled".to_string())),
        )
        .await;

        // 发送状态更新事件
        self.notify_status(request_id, HitlRequestStatus::Cancelled, "请求已取消");

        info!(request_id = %request_id, "HITL request cancelled");

        Ok(())
    }

    /// 取消指定会话的所有待处理请求
    ///
    /// 当 SSE 连接断开时调用此方法，清理该会话的所有 HITL 请求。
    /// 这样可以防止资源泄漏和孤儿请求累积。
    pub async fn cancel_by_conversation(&self, conversation_id: &str, reason: &str) -> usize {
        let pending_requests = self.store.get_pending_by_conversation(conversation_id);
        let count = pending_requests.len();

        if count == 0 {
            return 0;
        }

        info!(
            conversation_id = %conversation_id,
            count = count,
            reason = %reason,
            "Cancelling all pending HITL requests for conversation"
        );

        for request in pending_requests {
            // 更新状态为已取消
            if self
                .store
                .update_status(&request.id, HitlRequestStatus::Cancelled)
                .is_ok()
            {
                // 通知等待者
                self.notify_waiter(
                    &request.id,
                    Err(HitlError::RequestNotFound(format!(
                        "Request cancelled: {}",
                        reason
                    ))),
                )
                .await;

                // 发送状态更新事件
                self.notify_status(&request.id, HitlRequestStatus::Cancelled, reason);

                debug!(
                    request_id = %request.id,
                    conversation_id = %conversation_id,
                    "HITL request cancelled due to connection close"
                );
            }
        }

        count
    }

    /// 处理超时
    pub async fn process_timeouts(&self) -> Vec<HitlRequest> {
        let expired = self.store.get_expired();
        let mut processed = Vec::new();

        for request in expired {
            // 获取有效的超时行为
            let timeout_behavior =
                if let HitlRequestType::Confirmation(ref conf) = request.request_type {
                    request.timeout_behavior.effective_for_risk(conf.risk_level)
                } else {
                    request.timeout_behavior
                };

            // 根据超时行为处理
            let (new_status, response) = match timeout_behavior {
                TimeoutBehavior::Approve => {
                    (HitlRequestStatus::Approved, Some(HitlResponse::Approve))
                }
                TimeoutBehavior::Reject => (
                    HitlRequestStatus::TimedOut,
                    Some(HitlResponse::Reject {
                        reason: Some("请求超时".to_string()),
                    }),
                ),
                TimeoutBehavior::Skip => (HitlRequestStatus::Cancelled, None),
                TimeoutBehavior::Abort => (
                    HitlRequestStatus::Cancelled,
                    Some(HitlResponse::Abort {
                        reason: Some("请求超时".to_string()),
                    }),
                ),
                TimeoutBehavior::Wait => continue, // 不处理无限等待的请求
            };

            // 更新状态
            if let Ok(updated) = self.store.update(&request.id, |r| {
                r.status = new_status;
                r.response = response.clone();
            }) {
                // 通知等待者
                let waiter_response = response
                    .clone()
                    .ok_or_else(|| HitlError::Timeout(format!("Request {} timed out", request.id)));

                self.notify_waiter(&request.id, waiter_response).await;

                // 发送状态更新事件
                self.notify_status(&request.id, new_status, "请求超时");

                processed.push(updated);

                warn!(
                    request_id = %request.id,
                    timeout_behavior = ?timeout_behavior,
                    "HITL request timed out"
                );
            }
        }

        processed
    }

    /// 发送超时警告
    ///
    /// 跳过 `Wait` 行为的请求，因为它们会无限等待用户响应。
    pub fn send_timeout_warnings(&self) {
        let expiring = self
            .store
            .get_expiring_soon(self.timeout_warning_before_secs);

        for request in expiring {
            // 跳过 Wait 行为的请求 - 它们不需要超时警告
            if request.timeout_behavior == TimeoutBehavior::Wait {
                continue;
            }

            let remaining = request.remaining_seconds() as u64;
            self.notify_timeout_warning(&request.id, remaining);

            debug!(
                request_id = %request.id,
                remaining_seconds = remaining,
                "Sending timeout warning"
            );
        }
    }

    /// 启动超时处理定时器
    pub fn start_timeout_processor(self: Arc<Self>) -> JoinHandle<()> {
        let interval_secs = self.timeout_check_interval_secs;

        tokio::spawn(async move {
            let mut ticker = interval(Duration::from_secs(interval_secs));

            loop {
                ticker.tick().await;

                // 处理超时请求
                self.process_timeouts().await;

                // 发送超时警告
                self.send_timeout_warnings();

                // 清理已完成的请求
                let cleaned = self.store.cleanup_completed();
                if cleaned > 0 {
                    debug!(cleaned = cleaned, "Cleaned up completed requests");
                }
            }
        })
    }

    /// 获取待处理请求列表
    pub fn get_pending_requests(
        &self,
        conversation_id: Option<&str>,
        user_id: Option<&str>,
        limit: usize,
    ) -> Vec<HitlRequest> {
        let mut requests = if let Some(conv_id) = conversation_id {
            self.store.get_pending_by_conversation(conv_id)
        } else if let Some(uid) = user_id {
            self.store.get_pending_by_user(uid)
        } else {
            self.store.get_all_pending()
        };

        // 按创建时间排序（最新在前）
        requests.sort_by_key(|r| std::cmp::Reverse(r.created_at));

        // 限制数量
        requests.truncate(limit);

        requests
    }

    /// 获取请求详情
    pub fn get_request(&self, request_id: &str) -> Option<HitlRequest> {
        self.store.get(request_id)
    }

    /// 验证响应是否匹配请求类型
    fn validate_response(
        &self,
        request_type: &HitlRequestType,
        response: &HitlResponse,
    ) -> Result<(), HitlError> {
        match (request_type, response) {
            // 确认请求允许：Approve, Reject, Modify, Abort
            (
                HitlRequestType::Confirmation(_),
                HitlResponse::Approve
                | HitlResponse::Reject { .. }
                | HitlResponse::Modify { .. }
                | HitlResponse::Abort { .. },
            ) => Ok(()),

            // 澄清请求允许：Clarify, Abort
            (
                HitlRequestType::Clarification(_),
                HitlResponse::Clarify { .. } | HitlResponse::Abort { .. },
            ) => Ok(()),

            // 反馈请求允许：ProvideFeedback, Abort
            (
                HitlRequestType::Feedback(_),
                HitlResponse::ProvideFeedback { .. } | HitlResponse::Abort { .. },
            ) => Ok(()),

            // 暂停请求允许：Resume, Abort
            (HitlRequestType::Pause(_), HitlResponse::Resume | HitlResponse::Abort { .. }) => {
                Ok(())
            }

            // 隐私模式确认请求允许：PrivacyModeChoice, Abort
            (
                HitlRequestType::PrivacyModeConfirmation(_),
                HitlResponse::PrivacyModeChoice { .. } | HitlResponse::Abort { .. },
            ) => Ok(()),

            _ => Err(HitlError::InvalidResponse(format!(
                "Response {:?} is not valid for request type",
                response
            ))),
        }
    }

    /// 将响应转换为状态
    fn response_to_status(&self, response: &HitlResponse) -> HitlRequestStatus {
        match response {
            HitlResponse::Approve | HitlResponse::Resume => HitlRequestStatus::Approved,
            HitlResponse::Reject { .. } => HitlRequestStatus::Rejected,
            HitlResponse::Modify { .. } => HitlRequestStatus::Modified,
            HitlResponse::Abort { .. } => HitlRequestStatus::Cancelled,
            HitlResponse::Clarify { .. }
            | HitlResponse::ProvideFeedback { .. }
            | HitlResponse::PrivacyModeChoice { .. } => HitlRequestStatus::Completed,
        }
    }

    /// 通知等待者
    async fn notify_waiter(&self, request_id: &str, result: Result<HitlResponse, HitlError>) {
        let mut waiters = self.waiters.write().await;
        if let Some(tx) = waiters.remove(request_id) {
            let _ = tx.send(result);
        }
    }

    /// 发送请求事件
    fn notify_request(&self, request: &HitlRequest) {
        match self
            .event_sender
            .send(HitlEvent::Request(Box::new(request.clone())))
        {
            Ok(num_receivers) => {
                debug!(
                    request_id = %request.id,
                    num_receivers = num_receivers,
                    "HITL event broadcasted to subscribers"
                );
            }
            Err(e) => {
                warn!(
                    request_id = %request.id,
                    error = %e,
                    "Failed to broadcast HITL event (no subscribers?)"
                );
            }
        }
    }

    /// 发送状态更新事件
    fn notify_status(&self, request_id: &str, status: HitlRequestStatus, message: &str) {
        let _ = self.event_sender.send(HitlEvent::Status {
            request_id: request_id.to_string(),
            status,
            message: message.to_string(),
        });
    }

    /// 发送超时警告事件
    fn notify_timeout_warning(&self, request_id: &str, remaining_seconds: u64) {
        let _ = self.event_sender.send(HitlEvent::TimeoutWarning {
            request_id: request_id.to_string(),
            remaining_seconds,
        });
    }

    /// 获取配置
    pub fn config(&self) -> &HitlConfig {
        &self.config
    }

    /// 获取风险评估器
    pub fn risk_assessor(&self) -> &Arc<RiskAssessor> {
        &self.risk_assessor
    }

    /// 获取待处理请求数
    pub fn pending_count(&self) -> usize {
        self.store.count_pending()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::services::hitl::types::{GenericPreview, RiskLevel};

    fn create_test_manager() -> HitlManager {
        let config = HitlConfig::default();
        HitlManager::new(config, 100)
    }

    #[tokio::test]
    async fn test_create_request() {
        let manager = create_test_manager();

        let request = manager
            .create_request(
                HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                    summary: "Test operation".to_string(),
                    risk_level: RiskLevel::High,
                    tool_name: "test_tool".to_string(),
                    tool_args: json!({}),
                    preview: OperationPreview::Generic(GenericPreview {
                        title: "Test".to_string(),
                        description: "Test description".to_string(),
                        details: HashMap::new(),
                    }),
                    risk_factors: vec![],
                    allow_modification: false,
                    modifiable_fields: vec![],
                })),
                "conv1",
                "user1",
                300,
                TimeoutBehavior::Reject,
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        assert!(request.id.starts_with("hitl_"));
        assert_eq!(request.status, HitlRequestStatus::Pending);
        assert_eq!(request.conversation_id, "conv1");
        assert_eq!(request.user_id, "user1");
    }

    #[tokio::test]
    async fn test_handle_response() {
        let manager = create_test_manager();

        let request = manager
            .create_request(
                HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                    summary: "Test".to_string(),
                    risk_level: RiskLevel::High,
                    tool_name: "test".to_string(),
                    tool_args: json!({}),
                    preview: OperationPreview::Generic(GenericPreview {
                        title: "Test".to_string(),
                        description: "Test".to_string(),
                        details: HashMap::new(),
                    }),
                    risk_factors: vec![],
                    allow_modification: false,
                    modifiable_fields: vec![],
                })),
                "conv1",
                "user1",
                300,
                TimeoutBehavior::Reject,
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        let updated = manager
            .handle_response(&request.id, "user1", HitlResponse::Approve)
            .await
            .unwrap();

        assert_eq!(updated.status, HitlRequestStatus::Approved);
        assert!(matches!(updated.response, Some(HitlResponse::Approve)));
    }

    #[tokio::test]
    async fn test_handle_response_wrong_user() {
        let manager = create_test_manager();

        let request = manager
            .create_request(
                HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                    summary: "Test".to_string(),
                    risk_level: RiskLevel::High,
                    tool_name: "test".to_string(),
                    tool_args: json!({}),
                    preview: OperationPreview::Generic(GenericPreview {
                        title: "Test".to_string(),
                        description: "Test".to_string(),
                        details: HashMap::new(),
                    }),
                    risk_factors: vec![],
                    allow_modification: false,
                    modifiable_fields: vec![],
                })),
                "conv1",
                "user1",
                300,
                TimeoutBehavior::Reject,
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        let result = manager
            .handle_response(&request.id, "user2", HitlResponse::Approve)
            .await;

        assert!(matches!(result, Err(HitlError::PermissionDenied(_))));
    }

    #[tokio::test]
    async fn test_cancel_request() {
        let manager = create_test_manager();

        let request = manager
            .create_request(
                HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                    summary: "Test".to_string(),
                    risk_level: RiskLevel::High,
                    tool_name: "test".to_string(),
                    tool_args: json!({}),
                    preview: OperationPreview::Generic(GenericPreview {
                        title: "Test".to_string(),
                        description: "Test".to_string(),
                        details: HashMap::new(),
                    }),
                    risk_factors: vec![],
                    allow_modification: false,
                    modifiable_fields: vec![],
                })),
                "conv1",
                "user1",
                300,
                TimeoutBehavior::Reject,
                HashMap::new(),
                None,
                None,
            )
            .await
            .unwrap();

        manager.cancel_request(&request.id, "user1").await.unwrap();

        let updated = manager.get_request(&request.id).unwrap();
        assert_eq!(updated.status, HitlRequestStatus::Cancelled);
    }

    #[tokio::test]
    async fn test_validate_response() {
        let manager = create_test_manager();

        // 确认请求 + Approve = OK
        assert!(
            manager
                .validate_response(
                    &HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                        summary: "Test".to_string(),
                        risk_level: RiskLevel::High,
                        tool_name: "test".to_string(),
                        tool_args: json!({}),
                        preview: OperationPreview::Generic(GenericPreview {
                            title: "Test".to_string(),
                            description: "Test".to_string(),
                            details: HashMap::new(),
                        }),
                        risk_factors: vec![],
                        allow_modification: false,
                        modifiable_fields: vec![],
                    })),
                    &HitlResponse::Approve,
                )
                .is_ok()
        );

        // 确认请求 + Clarify = Error
        assert!(
            manager
                .validate_response(
                    &HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                        summary: "Test".to_string(),
                        risk_level: RiskLevel::High,
                        tool_name: "test".to_string(),
                        tool_args: json!({}),
                        preview: OperationPreview::Generic(GenericPreview {
                            title: "Test".to_string(),
                            description: "Test".to_string(),
                            details: HashMap::new(),
                        }),
                        risk_factors: vec![],
                        allow_modification: false,
                        modifiable_fields: vec![],
                    })),
                    &HitlResponse::Clarify {
                        selected_option: None,
                        input: Some("test".to_string()),
                    },
                )
                .is_err()
        );
    }

    #[tokio::test]
    async fn test_get_pending_requests() {
        let manager = create_test_manager();

        // 创建多个请求
        for i in 0..5 {
            manager
                .create_request(
                    HitlRequestType::Confirmation(Box::new(ConfirmationRequest {
                        summary: format!("Test {}", i),
                        risk_level: RiskLevel::High,
                        tool_name: "test".to_string(),
                        tool_args: json!({}),
                        preview: OperationPreview::Generic(GenericPreview {
                            title: "Test".to_string(),
                            description: "Test".to_string(),
                            details: HashMap::new(),
                        }),
                        risk_factors: vec![],
                        allow_modification: false,
                        modifiable_fields: vec![],
                    })),
                    "conv1",
                    "user1",
                    300,
                    TimeoutBehavior::Reject,
                    HashMap::new(),
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        let pending = manager.get_pending_requests(Some("conv1"), None, 10);
        assert_eq!(pending.len(), 5);

        let pending = manager.get_pending_requests(Some("conv1"), None, 3);
        assert_eq!(pending.len(), 3);
    }

    #[tokio::test]
    async fn test_assess_and_create_confirmation() {
        let manager = create_test_manager();

        let assessment = manager.assess_risk("delete_file", &json!({ "path": "/tmp/test.txt" }));

        if assessment.requires_confirmation {
            let request = manager
                .create_confirmation_request(
                    "delete_file",
                    &json!({ "path": "/tmp/test.txt" }),
                    &assessment,
                    OperationPreview::Generic(GenericPreview {
                        title: "Delete File".to_string(),
                        description: "Delete /tmp/test.txt".to_string(),
                        details: HashMap::new(),
                    }),
                    "conv1",
                    "user1",
                    None,
                    None,
                )
                .await
                .unwrap();

            assert!(request.id.starts_with("hitl_"));
        }
    }

    #[test]
    fn test_response_to_status() {
        let manager = create_test_manager();

        assert_eq!(
            manager.response_to_status(&HitlResponse::Approve),
            HitlRequestStatus::Approved
        );
        assert_eq!(
            manager.response_to_status(&HitlResponse::Reject { reason: None }),
            HitlRequestStatus::Rejected
        );
        assert_eq!(
            manager.response_to_status(&HitlResponse::Modify {
                modifications: json!({})
            }),
            HitlRequestStatus::Modified
        );
        assert_eq!(
            manager.response_to_status(&HitlResponse::Resume),
            HitlRequestStatus::Approved
        );
        assert_eq!(
            manager.response_to_status(&HitlResponse::Abort { reason: None }),
            HitlRequestStatus::Cancelled
        );
    }
}
