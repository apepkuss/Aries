//! HITL 核心类型定义
//!
//! 本模块定义了 Human-in-the-Loop 机制的所有核心数据类型。

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// HITL 请求 ID
pub type HitlRequestId = String;

/// HITL 请求类型
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HitlRequestType {
    /// 操作确认
    Confirmation(Box<ConfirmationRequest>),
    /// 澄清请求
    Clarification(ClarificationRequest),
    /// 反馈收集
    Feedback(FeedbackRequest),
    /// 执行暂停
    Pause(PauseRequest),
    /// 隐私模式确认
    PrivacyModeConfirmation(PrivacyModeConfirmationRequest),
}

/// HITL 请求状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitlRequestStatus {
    /// 等待用户响应
    Pending,
    /// 用户已批准
    Approved,
    /// 用户已拒绝
    Rejected,
    /// 用户已修改
    Modified,
    /// 已超时
    TimedOut,
    /// 已取消（系统或用户）
    Cancelled,
    /// 处理中（正在应用用户响应）
    Processing,
    /// 已完成
    Completed,
}

impl HitlRequestStatus {
    /// 是否为终态
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Rejected | Self::Cancelled | Self::Completed | Self::TimedOut
        )
    }

    /// 是否可以接受响应
    pub fn can_respond(&self) -> bool {
        matches!(self, Self::Pending)
    }

    /// 转换为 snake_case 字符串（与 serde 序列化一致）
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Modified => "modified",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Processing => "processing",
            Self::Completed => "completed",
        }
    }
}

/// HITL 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlRequest {
    /// 唯一标识
    pub id: HitlRequestId,
    /// 请求类型和详情
    pub request_type: HitlRequestType,
    /// 当前状态
    pub status: HitlRequestStatus,
    /// 关联的对话 ID
    pub conversation_id: String,
    /// 关联的用户 ID
    pub user_id: String,
    /// 关联的子任务 ID（如有）
    pub subtask_id: Option<usize>,
    /// 关联的 Sub-Agent ID（如有）
    pub subagent_id: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 更新时间
    pub updated_at: DateTime<Utc>,
    /// 过期时间
    pub expires_at: DateTime<Utc>,
    /// 超时后的默认行为
    pub timeout_behavior: TimeoutBehavior,
    /// 用户响应（如有）
    pub response: Option<HitlResponse>,
    /// 元数据
    pub metadata: HashMap<String, serde_json::Value>,
}

impl HitlRequest {
    /// 创建新的 HITL 请求
    pub fn new(
        id: HitlRequestId,
        request_type: HitlRequestType,
        conversation_id: String,
        user_id: String,
        timeout_secs: u64,
        timeout_behavior: TimeoutBehavior,
    ) -> Self {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::seconds(timeout_secs as i64);

        Self {
            id,
            request_type,
            status: HitlRequestStatus::Pending,
            conversation_id,
            user_id,
            subtask_id: None,
            subagent_id: None,
            created_at: now,
            updated_at: now,
            expires_at,
            timeout_behavior,
            response: None,
            metadata: HashMap::new(),
        }
    }

    /// 检查请求是否已过期
    ///
    /// 对于 `Wait` 行为的请求，永远返回 `false`，因为它们会无限等待用户响应。
    pub fn is_expired(&self) -> bool {
        // Wait 行为的请求永不过期
        if self.timeout_behavior == TimeoutBehavior::Wait {
            return false;
        }
        Utc::now() > self.expires_at
    }

    /// 获取剩余时间（秒）
    pub fn remaining_seconds(&self) -> i64 {
        let remaining = self.expires_at - Utc::now();
        remaining.num_seconds().max(0)
    }

    /// 获取风险级别（仅对确认请求有效）
    pub fn risk_level(&self) -> Option<RiskLevel> {
        match &self.request_type {
            HitlRequestType::Confirmation(conf) => Some(conf.risk_level),
            _ => None,
        }
    }
}

/// 超时行为
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimeoutBehavior {
    /// 自动拒绝
    Reject,
    /// 自动批准（仅限低风险，High/Critical 会被强制改为 Reject）
    Approve,
    /// 跳过此操作继续执行
    Skip,
    /// 终止整个任务
    Abort,
    /// 无限等待（不超时）- 默认
    #[default]
    Wait,
}

impl TimeoutBehavior {
    /// 获取有效的超时行为（考虑风险级别）
    pub fn effective_for_risk(&self, risk_level: RiskLevel) -> Self {
        // High/Critical 级别不允许自动批准
        if *self == Self::Approve && risk_level >= RiskLevel::High {
            Self::Reject
        } else {
            *self
        }
    }
}

/// 风险级别
///
/// 注意：Safe 级别只能通过 L3 运行时信任获得，L1/L2 不能声明 Safe
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// 安全操作 - 无副作用，自动执行
    /// ⚠️ 只能通过 L3 运行时信任获得，L1/L2 不允许直接声明
    Safe,
    /// 低风险 - 有副作用但影响有限，通知用户但自动执行
    #[default]
    Low,
    /// 中等风险 - 需要用户确认
    Medium,
    /// 高风险 - 必须用户确认，显示详细预览
    High,
    /// 关键操作 - 需要额外验证（如密码确认）
    Critical,
}

impl RiskLevel {
    /// 是否需要确认（基于阈值）
    pub fn requires_confirmation(&self, threshold: RiskLevel) -> bool {
        *self >= threshold
    }

    /// 降低一级风险
    pub fn decrease_one_level(&self) -> Self {
        match self {
            Self::Critical => Self::High,
            Self::High => Self::Medium,
            Self::Medium => Self::Low,
            Self::Low => Self::Safe,
            Self::Safe => Self::Safe,
        }
    }

    /// 是否可以在 L1/L2 中声明（Safe 不允许）
    pub fn is_declarable(&self) -> bool {
        *self != Self::Safe
    }
}

/// 确认请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmationRequest {
    /// 操作摘要（一句话描述）
    pub summary: String,
    /// 风险级别
    pub risk_level: RiskLevel,
    /// 工具名称
    pub tool_name: String,
    /// 工具参数
    pub tool_args: serde_json::Value,
    /// 操作预览（结构化）
    pub preview: OperationPreview,
    /// 风险因素说明
    pub risk_factors: Vec<RiskFactor>,
    /// 是否允许修改参数
    pub allow_modification: bool,
    /// 可修改的字段
    pub modifiable_fields: Vec<String>,
}

/// 操作预览
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OperationPreview {
    /// 邮件预览
    Email(EmailPreview),
    /// 文件操作预览
    FileOperation(FileOperationPreview),
    /// Shell 命令预览
    ShellCommand(ShellCommandPreview),
    /// HTTP 请求预览
    HttpRequest(HttpRequestPreview),
    /// 通用预览
    Generic(GenericPreview),
}

/// 邮件预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailPreview {
    pub to: Vec<String>,
    pub cc: Option<Vec<String>>,
    pub bcc: Option<Vec<String>>,
    pub subject: String,
    pub body: String,
    pub body_format: EmailBodyFormat,
    pub attachments: Vec<AttachmentInfo>,
    pub is_reply: bool,
    pub reply_to_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailBodyFormat {
    PlainText,
    Html,
    Markdown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentInfo {
    pub filename: String,
    pub size_bytes: u64,
    pub mime_type: String,
}

/// 文件操作预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOperationPreview {
    pub operation: FileOperationType,
    pub path: String,
    pub content_preview: Option<String>,
    pub size_bytes: Option<u64>,
    pub affected_files_count: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileOperationType {
    Create,
    Modify,
    Delete,
    Move,
    Copy,
}

/// Shell 命令预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellCommandPreview {
    pub command: String,
    pub working_directory: Option<String>,
    pub environment: Option<HashMap<String, String>>,
    pub estimated_impact: String,
}

/// HTTP 请求预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequestPreview {
    pub method: String,
    pub url: String,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<String>,
    pub is_external: bool,
}

/// 通用预览
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericPreview {
    pub title: String,
    pub description: String,
    pub details: HashMap<String, serde_json::Value>,
}

/// 风险因素
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskFactor {
    pub code: String,
    pub description: String,
    pub severity: RiskFactorSeverity,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskFactorSeverity {
    Info,
    Warning,
    Danger,
}

/// 澄清请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationRequest {
    /// 需要澄清的问题
    pub question: String,
    /// 问题上下文
    pub context: String,
    /// 建议的选项（可选）
    pub options: Option<Vec<ClarificationOption>>,
    /// 是否允许自由输入
    pub allow_free_input: bool,
    /// 输入提示
    pub input_placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClarificationOption {
    pub id: String,
    pub label: String,
    pub description: Option<String>,
}

/// 反馈请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRequest {
    /// 反馈主题
    pub subject: String,
    /// 相关操作描述
    pub operation_summary: String,
    /// 评分类型
    pub rating_type: RatingType,
    /// 是否需要文字反馈
    pub require_comment: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RatingType {
    /// 好/坏
    Binary,
    /// 1-5 星
    Stars,
    /// 满意度量表
    Satisfaction,
}

/// 暂停请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PauseRequest {
    /// 暂停原因
    pub reason: PauseReason,
    /// 当前状态摘要
    pub current_state: String,
    /// 已完成步骤
    pub completed_steps: Vec<String>,
    /// 待执行步骤
    pub pending_steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    /// 用户主动暂停
    UserRequested,
    /// 达到检查点
    Checkpoint,
    /// 检测到异常
    AnomalyDetected,
    /// 资源限制
    ResourceLimit,
}

/// 隐私模式确认请求
///
/// 当系统检测到用户查询可能包含隐私信息时，发送此请求让用户确认是否使用隐私模式。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyModeConfirmationRequest {
    /// 用户查询摘要（已脱敏）
    pub query_summary: String,
    /// 检测到的隐私模式列表
    pub detected_patterns: Vec<DetectedPrivacyPattern>,
    /// 检测方法
    pub detection_method: String,
    /// 综合置信度 (0.0 - 1.0)
    pub confidence: f32,
    /// 建议说明
    pub recommendation: String,
}

/// 检测到的隐私模式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPrivacyPattern {
    /// 隐私类别
    pub category: String,
    /// 类别描述
    pub description: String,
    /// 匹配到的文本（已脱敏，可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub masked_text: Option<String>,
}

/// HITL 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HitlResponse {
    /// 批准执行
    Approve,
    /// 拒绝执行
    Reject { reason: Option<String> },
    /// 修改后批准
    Modify { modifications: serde_json::Value },
    /// 提供澄清信息
    Clarify {
        /// 选择的选项 ID（如有）
        selected_option: Option<String>,
        /// 自由输入内容（如有）
        input: Option<String>,
    },
    /// 提供反馈
    ProvideFeedback {
        rating: Option<i32>,
        comment: Option<String>,
    },
    /// 继续执行（从暂停恢复）
    Resume,
    /// 终止执行
    Abort { reason: Option<String> },
    /// 隐私模式选择
    PrivacyModeChoice {
        /// 是否使用隐私模式
        use_privacy_mode: bool,
        /// 是否记住本次会话的选择
        #[serde(default)]
        remember_choice: bool,
    },
}

/// HITL 错误类型
#[derive(Debug, Clone, thiserror::Error)]
pub enum HitlError {
    #[error("HITL request not found: {0}")]
    RequestNotFound(String),

    #[error("HITL request has expired: {0}")]
    RequestExpired(String),

    #[error("Invalid response for request type: {0}")]
    InvalidResponse(String),

    #[error("Request is not in pending status: {0}")]
    NotPending(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Store error: {0}")]
    StoreError(String),

    #[error("Notification error: {0}")]
    NotificationError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("HITL request cancelled: {0}")]
    Cancelled(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_level_ordering() {
        assert!(RiskLevel::Safe < RiskLevel::Low);
        assert!(RiskLevel::Low < RiskLevel::Medium);
        assert!(RiskLevel::Medium < RiskLevel::High);
        assert!(RiskLevel::High < RiskLevel::Critical);
    }

    #[test]
    fn test_risk_level_decrease() {
        assert_eq!(RiskLevel::Critical.decrease_one_level(), RiskLevel::High);
        assert_eq!(RiskLevel::High.decrease_one_level(), RiskLevel::Medium);
        assert_eq!(RiskLevel::Medium.decrease_one_level(), RiskLevel::Low);
        assert_eq!(RiskLevel::Low.decrease_one_level(), RiskLevel::Safe);
        assert_eq!(RiskLevel::Safe.decrease_one_level(), RiskLevel::Safe);
    }

    #[test]
    fn test_risk_level_requires_confirmation() {
        let threshold = RiskLevel::Medium;

        assert!(!RiskLevel::Safe.requires_confirmation(threshold));
        assert!(!RiskLevel::Low.requires_confirmation(threshold));
        assert!(RiskLevel::Medium.requires_confirmation(threshold));
        assert!(RiskLevel::High.requires_confirmation(threshold));
        assert!(RiskLevel::Critical.requires_confirmation(threshold));
    }

    #[test]
    fn test_risk_level_is_declarable() {
        assert!(!RiskLevel::Safe.is_declarable());
        assert!(RiskLevel::Low.is_declarable());
        assert!(RiskLevel::Medium.is_declarable());
        assert!(RiskLevel::High.is_declarable());
        assert!(RiskLevel::Critical.is_declarable());
    }

    #[test]
    fn test_timeout_behavior_default() {
        assert_eq!(TimeoutBehavior::default(), TimeoutBehavior::Wait);
    }

    #[test]
    fn test_timeout_behavior_effective_for_risk() {
        // Approve 对于 Low/Medium 有效
        assert_eq!(
            TimeoutBehavior::Approve.effective_for_risk(RiskLevel::Low),
            TimeoutBehavior::Approve
        );
        assert_eq!(
            TimeoutBehavior::Approve.effective_for_risk(RiskLevel::Medium),
            TimeoutBehavior::Approve
        );

        // Approve 对于 High/Critical 被强制改为 Reject
        assert_eq!(
            TimeoutBehavior::Approve.effective_for_risk(RiskLevel::High),
            TimeoutBehavior::Reject
        );
        assert_eq!(
            TimeoutBehavior::Approve.effective_for_risk(RiskLevel::Critical),
            TimeoutBehavior::Reject
        );

        // 其他行为不受风险级别影响
        assert_eq!(
            TimeoutBehavior::Reject.effective_for_risk(RiskLevel::Critical),
            TimeoutBehavior::Reject
        );
        assert_eq!(
            TimeoutBehavior::Skip.effective_for_risk(RiskLevel::Critical),
            TimeoutBehavior::Skip
        );
    }

    #[test]
    fn test_hitl_request_status_is_terminal() {
        assert!(!HitlRequestStatus::Pending.is_terminal());
        assert!(!HitlRequestStatus::Approved.is_terminal());
        assert!(!HitlRequestStatus::Processing.is_terminal());

        assert!(HitlRequestStatus::Rejected.is_terminal());
        assert!(HitlRequestStatus::Cancelled.is_terminal());
        assert!(HitlRequestStatus::Completed.is_terminal());
        assert!(HitlRequestStatus::TimedOut.is_terminal());
    }

    #[test]
    fn test_hitl_request_status_can_respond() {
        assert!(HitlRequestStatus::Pending.can_respond());

        assert!(!HitlRequestStatus::Approved.can_respond());
        assert!(!HitlRequestStatus::Rejected.can_respond());
        assert!(!HitlRequestStatus::Processing.can_respond());
        assert!(!HitlRequestStatus::Completed.can_respond());
    }

    #[test]
    fn test_hitl_response_serialization() {
        let approve = HitlResponse::Approve;
        let json = serde_json::to_string(&approve).unwrap();
        assert!(json.contains("approve"));

        let reject = HitlResponse::Reject {
            reason: Some("Test".to_string()),
        };
        let json = serde_json::to_string(&reject).unwrap();
        assert!(json.contains("reject"));
        assert!(json.contains("Test"));
    }

    #[test]
    fn test_privacy_mode_confirmation_request_serialization() {
        let request = PrivacyModeConfirmationRequest {
            query_summary: "查询手机号...".to_string(),
            detected_patterns: vec![DetectedPrivacyPattern {
                category: "contact".to_string(),
                description: "联系方式".to_string(),
                masked_text: Some("138****5678".to_string()),
            }],
            detection_method: "rule_based".to_string(),
            confidence: 0.9,
            recommendation: "建议使用隐私模式".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("query_summary"));
        assert!(json.contains("detected_patterns"));
        assert!(json.contains("contact"));
        assert!(json.contains("138****5678"));

        // Test deserialization
        let parsed: PrivacyModeConfirmationRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.detected_patterns.len(), 1);
        assert!((parsed.confidence - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn test_privacy_mode_choice_response_serialization() {
        let response = HitlResponse::PrivacyModeChoice {
            use_privacy_mode: true,
            remember_choice: false,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("privacy_mode_choice"));
        assert!(json.contains("use_privacy_mode"));
        assert!(json.contains("true"));

        // Test deserialization
        let parsed: HitlResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            HitlResponse::PrivacyModeChoice {
                use_privacy_mode,
                remember_choice,
            } => {
                assert!(use_privacy_mode);
                assert!(!remember_choice);
            }
            _ => panic!("Expected PrivacyModeChoice"),
        }
    }

    #[test]
    fn test_hitl_request_type_privacy_mode_confirmation() {
        let request_type =
            HitlRequestType::PrivacyModeConfirmation(PrivacyModeConfirmationRequest {
                query_summary: "test".to_string(),
                detected_patterns: vec![],
                detection_method: "rule_based".to_string(),
                confidence: 0.8,
                recommendation: "test".to_string(),
            });

        let json = serde_json::to_string(&request_type).unwrap();
        assert!(json.contains("privacy_mode_confirmation"));

        let parsed: HitlRequestType = serde_json::from_str(&json).unwrap();
        matches!(parsed, HitlRequestType::PrivacyModeConfirmation(_));
    }
}
