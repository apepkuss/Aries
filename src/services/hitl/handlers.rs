//! HITL HTTP API 处理器
//!
//! 此模块实现 Human-in-the-Loop 的 HTTP API 端点。
//!
//! # 端点
//!
//! - `GET /api/hitl/pending` - 列出待处理请求
//! - `GET /api/hitl/requests/{id}` - 获取请求详情
//! - `POST /api/hitl/requests/{id}/respond` - 响应请求
//! - `DELETE /api/hitl/requests/{id}` - 取消请求
//! - `GET /api/hitl/stats` - 获取统计信息

use std::{collections::HashMap, sync::Arc};

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::{
    manager::HitlManager,
    types::{
        HitlRequest, HitlRequestStatus, HitlResponse, OperationPreview, RiskFactor, RiskLevel,
    },
};

// ============================================================================
// Request/Response Types
// ============================================================================

/// 列表查询参数
#[derive(Debug, Deserialize)]
pub struct ListPendingQuery {
    /// 按对话 ID 筛选
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// 按用户 ID 筛选
    #[serde(default)]
    pub user_id: Option<String>,
    /// 限制返回数量
    #[serde(default = "default_limit")]
    pub limit: usize,
}

fn default_limit() -> usize {
    50
}

/// 待处理请求列表响应
#[derive(Debug, Serialize)]
pub struct ListPendingResponse {
    /// 请求列表
    pub requests: Vec<HitlRequestSummary>,
    /// 总数
    pub total: usize,
}

/// HITL 请求摘要（用于列表展示）
#[derive(Debug, Serialize)]
pub struct HitlRequestSummary {
    /// 请求 ID
    pub id: String,
    /// 请求类型
    pub request_type: String,
    /// 状态
    pub status: HitlRequestStatus,
    /// 对话 ID
    pub conversation_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 风险级别（仅确认请求）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<RiskLevel>,
    /// 摘要描述
    pub summary: String,
    /// 创建时间
    pub created_at: String,
    /// 过期时间
    pub expires_at: String,
    /// 剩余秒数
    pub remaining_seconds: i64,
}

impl From<&HitlRequest> for HitlRequestSummary {
    fn from(request: &HitlRequest) -> Self {
        let (request_type, risk_level, summary) = match &request.request_type {
            super::types::HitlRequestType::Confirmation(conf) => (
                "confirmation".to_string(),
                Some(conf.risk_level),
                conf.summary.clone(),
            ),
            super::types::HitlRequestType::Clarification(clar) => {
                ("clarification".to_string(), None, clar.question.clone())
            }
            super::types::HitlRequestType::Feedback(fb) => {
                ("feedback".to_string(), None, fb.subject.clone())
            }
            super::types::HitlRequestType::Pause(pause) => {
                ("pause".to_string(), None, format!("{:?}", pause.reason))
            }
        };

        Self {
            id: request.id.clone(),
            request_type,
            status: request.status,
            conversation_id: request.conversation_id.clone(),
            user_id: request.user_id.clone(),
            risk_level,
            summary,
            created_at: request.created_at.to_rfc3339(),
            expires_at: request.expires_at.to_rfc3339(),
            remaining_seconds: request.remaining_seconds(),
        }
    }
}

/// 请求详情响应
#[derive(Debug, Serialize)]
pub struct HitlRequestDetailResponse {
    /// 请求 ID
    pub id: String,
    /// 请求类型
    pub request_type: String,
    /// 状态
    pub status: HitlRequestStatus,
    /// 对话 ID
    pub conversation_id: String,
    /// 用户 ID
    pub user_id: String,
    /// 请求详情
    pub details: RequestDetails,
    /// 创建时间
    pub created_at: String,
    /// 更新时间
    pub updated_at: String,
    /// 过期时间
    pub expires_at: String,
    /// 剩余秒数
    pub remaining_seconds: i64,
    /// 超时行为
    pub timeout_behavior: String,
    /// 元数据
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub metadata: HashMap<String, serde_json::Value>,
    /// 响应（如果有）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<HitlResponse>,
}

/// 请求详情
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum RequestDetails {
    /// 确认请求详情
    Confirmation(Box<ConfirmationDetails>),
    /// 澄清请求详情
    Clarification(ClarificationDetails),
    /// 反馈请求详情
    Feedback(FeedbackDetails),
    /// 暂停请求详情
    Pause(PauseDetails),
}

/// 确认请求详情
#[derive(Debug, Serialize)]
pub struct ConfirmationDetails {
    /// 操作摘要
    pub summary: String,
    /// 风险级别
    pub risk_level: RiskLevel,
    /// 工具名称
    pub tool_name: String,
    /// 工具参数
    pub tool_args: serde_json::Value,
    /// 操作预览
    pub preview: OperationPreview,
    /// 风险因素
    pub risk_factors: Vec<RiskFactor>,
    /// 是否允许修改
    pub allow_modification: bool,
    /// 可修改字段
    pub modifiable_fields: Vec<String>,
}

/// 澄清请求详情
#[derive(Debug, Serialize)]
pub struct ClarificationDetails {
    /// 问题
    pub question: String,
    /// 上下文
    pub context: String,
    /// 选项
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<ClarificationOptionResponse>>,
    /// 是否允许自由输入
    pub allow_free_input: bool,
    /// 输入提示
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_placeholder: Option<String>,
}

/// 澄清选项响应
#[derive(Debug, Serialize)]
pub struct ClarificationOptionResponse {
    pub id: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// 反馈请求详情
#[derive(Debug, Serialize)]
pub struct FeedbackDetails {
    /// 反馈主题
    pub subject: String,
    /// 相关操作描述
    pub operation_summary: String,
    /// 评分类型
    pub rating_type: String,
    /// 是否需要文字反馈
    pub require_comment: bool,
}

/// 暂停请求详情
#[derive(Debug, Serialize)]
pub struct PauseDetails {
    /// 暂停原因
    pub reason: String,
    /// 当前状态
    pub current_state: String,
    /// 已完成步骤
    pub completed_steps: Vec<String>,
    /// 待执行步骤
    pub pending_steps: Vec<String>,
}

impl From<&HitlRequest> for HitlRequestDetailResponse {
    fn from(request: &HitlRequest) -> Self {
        let (request_type, details) = match &request.request_type {
            super::types::HitlRequestType::Confirmation(conf) => (
                "confirmation".to_string(),
                RequestDetails::Confirmation(Box::new(ConfirmationDetails {
                    summary: conf.summary.clone(),
                    risk_level: conf.risk_level,
                    tool_name: conf.tool_name.clone(),
                    tool_args: conf.tool_args.clone(),
                    preview: conf.preview.clone(),
                    risk_factors: conf.risk_factors.clone(),
                    allow_modification: conf.allow_modification,
                    modifiable_fields: conf.modifiable_fields.clone(),
                })),
            ),
            super::types::HitlRequestType::Clarification(clar) => (
                "clarification".to_string(),
                RequestDetails::Clarification(ClarificationDetails {
                    question: clar.question.clone(),
                    context: clar.context.clone(),
                    options: clar.options.as_ref().map(|opts| {
                        opts.iter()
                            .map(|opt| ClarificationOptionResponse {
                                id: opt.id.clone(),
                                label: opt.label.clone(),
                                description: opt.description.clone(),
                            })
                            .collect()
                    }),
                    allow_free_input: clar.allow_free_input,
                    input_placeholder: clar.input_placeholder.clone(),
                }),
            ),
            super::types::HitlRequestType::Feedback(fb) => (
                "feedback".to_string(),
                RequestDetails::Feedback(FeedbackDetails {
                    subject: fb.subject.clone(),
                    operation_summary: fb.operation_summary.clone(),
                    rating_type: format!("{:?}", fb.rating_type),
                    require_comment: fb.require_comment,
                }),
            ),
            super::types::HitlRequestType::Pause(pause) => (
                "pause".to_string(),
                RequestDetails::Pause(PauseDetails {
                    reason: format!("{:?}", pause.reason),
                    current_state: pause.current_state.clone(),
                    completed_steps: pause.completed_steps.clone(),
                    pending_steps: pause.pending_steps.clone(),
                }),
            ),
        };

        Self {
            id: request.id.clone(),
            request_type,
            status: request.status,
            conversation_id: request.conversation_id.clone(),
            user_id: request.user_id.clone(),
            details,
            created_at: request.created_at.to_rfc3339(),
            updated_at: request.updated_at.to_rfc3339(),
            expires_at: request.expires_at.to_rfc3339(),
            remaining_seconds: request.remaining_seconds(),
            timeout_behavior: format!("{:?}", request.timeout_behavior),
            metadata: request.metadata.clone(),
            response: request.response.clone(),
        }
    }
}

/// 响应请求体
#[derive(Debug, Deserialize)]
pub struct RespondRequest {
    /// 用户 ID
    pub user_id: String,
    /// 响应类型
    pub response_type: String,
    /// 响应数据
    #[serde(default)]
    pub data: Option<serde_json::Value>,
}

/// 响应结果
#[derive(Debug, Serialize)]
pub struct RespondResponse {
    /// 是否成功
    pub success: bool,
    /// 请求 ID
    pub request_id: String,
    /// 新状态
    pub status: HitlRequestStatus,
    /// 消息
    pub message: String,
}

/// 取消响应
#[derive(Debug, Serialize)]
pub struct CancelResponse {
    /// 是否成功
    pub success: bool,
    /// 请求 ID
    pub request_id: String,
    /// 消息
    pub message: String,
}

/// 统计信息响应
#[derive(Debug, Serialize)]
pub struct StatsResponse {
    /// 待处理请求数
    pub pending_count: usize,
    /// HITL 是否启用
    pub enabled: bool,
    /// 确认阈值
    pub confirmation_threshold: RiskLevel,
}

/// 错误响应
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// 错误代码
    pub code: String,
    /// 错误消息
    pub message: String,
}

impl ErrorResponse {
    pub fn not_found(id: &str) -> Self {
        Self {
            code: "NOT_FOUND".to_string(),
            message: format!("HITL request '{}' not found", id),
        }
    }

    pub fn permission_denied(msg: impl Into<String>) -> Self {
        Self {
            code: "PERMISSION_DENIED".to_string(),
            message: msg.into(),
        }
    }

    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: "INVALID_REQUEST".to_string(),
            message: msg.into(),
        }
    }

    pub fn expired(id: &str) -> Self {
        Self {
            code: "REQUEST_EXPIRED".to_string(),
            message: format!("HITL request '{}' has expired", id),
        }
    }

    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self {
            code: "INTERNAL_ERROR".to_string(),
            message: msg.into(),
        }
    }
}

// ============================================================================
// API State
// ============================================================================

/// HITL API 状态
#[derive(Clone)]
pub struct HitlApiState {
    pub manager: Arc<HitlManager>,
}

impl HitlApiState {
    pub fn new(manager: Arc<HitlManager>) -> Self {
        Self { manager }
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// 列出待处理请求
///
/// GET /api/hitl/pending
pub async fn list_pending_handler(
    State(state): State<HitlApiState>,
    Query(query): Query<ListPendingQuery>,
) -> impl IntoResponse {
    let limit = query.limit.min(100);

    let requests = state.manager.get_pending_requests(
        query.conversation_id.as_deref(),
        query.user_id.as_deref(),
        limit,
    );

    let total = requests.len();
    let summaries: Vec<HitlRequestSummary> =
        requests.iter().map(HitlRequestSummary::from).collect();

    let response = ListPendingResponse {
        requests: summaries,
        total,
    };

    (StatusCode::OK, Json(response))
}

/// 获取请求详情
///
/// GET /api/hitl/requests/{id}
pub async fn get_request_handler(
    State(state): State<HitlApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.manager.get_request(&id) {
        Some(request) => {
            let response = HitlRequestDetailResponse::from(&request);
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        None => {
            let error = ErrorResponse::not_found(&id);
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(error).unwrap()),
            )
                .into_response()
        }
    }
}

/// 响应请求
///
/// POST /api/hitl/requests/{id}/respond
pub async fn respond_handler(
    State(state): State<HitlApiState>,
    Path(id): Path<String>,
    Json(body): Json<RespondRequest>,
) -> impl IntoResponse {
    // 解析响应类型
    let response = match parse_response(&body.response_type, body.data) {
        Ok(r) => r,
        Err(e) => {
            let error = ErrorResponse::invalid_request(e);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::to_value(error).unwrap()),
            )
                .into_response();
        }
    };

    // 处理响应
    match state
        .manager
        .handle_response(&id, &body.user_id, response)
        .await
    {
        Ok(updated) => {
            let response = RespondResponse {
                success: true,
                request_id: id,
                status: updated.status,
                message: "Response recorded successfully".to_string(),
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            let (status, error) = match &e {
                super::types::HitlError::RequestNotFound(_) => {
                    (StatusCode::NOT_FOUND, ErrorResponse::not_found(&id))
                }
                super::types::HitlError::PermissionDenied(msg) => {
                    (StatusCode::FORBIDDEN, ErrorResponse::permission_denied(msg))
                }
                super::types::HitlError::RequestExpired(_) => {
                    (StatusCode::GONE, ErrorResponse::expired(&id))
                }
                super::types::HitlError::NotPending(_) => (
                    StatusCode::CONFLICT,
                    ErrorResponse::invalid_request("Request is no longer pending"),
                ),
                super::types::HitlError::InvalidResponse(msg) => {
                    (StatusCode::BAD_REQUEST, ErrorResponse::invalid_request(msg))
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::internal_error(e.to_string()),
                ),
            };
            (status, Json(serde_json::to_value(error).unwrap())).into_response()
        }
    }
}

/// 取消请求
///
/// DELETE /api/hitl/requests/{id}
pub async fn cancel_handler(
    State(state): State<HitlApiState>,
    Path(id): Path<String>,
    Query(query): Query<UserIdQuery>,
) -> impl IntoResponse {
    let user_id = match query.user_id {
        Some(uid) => uid,
        None => {
            let error = ErrorResponse::invalid_request("user_id query parameter is required");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::to_value(error).unwrap()),
            )
                .into_response();
        }
    };

    match state.manager.cancel_request(&id, &user_id).await {
        Ok(()) => {
            let response = CancelResponse {
                success: true,
                request_id: id,
                message: "Request cancelled successfully".to_string(),
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            let (status, error) = match &e {
                super::types::HitlError::RequestNotFound(_) => {
                    (StatusCode::NOT_FOUND, ErrorResponse::not_found(&id))
                }
                super::types::HitlError::PermissionDenied(msg) => {
                    (StatusCode::FORBIDDEN, ErrorResponse::permission_denied(msg))
                }
                super::types::HitlError::NotPending(_) => (
                    StatusCode::CONFLICT,
                    ErrorResponse::invalid_request("Request is no longer pending"),
                ),
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ErrorResponse::internal_error(e.to_string()),
                ),
            };
            (status, Json(serde_json::to_value(error).unwrap())).into_response()
        }
    }
}

/// 用户 ID 查询参数
#[derive(Debug, Deserialize)]
pub struct UserIdQuery {
    pub user_id: Option<String>,
}

/// 获取统计信息
///
/// GET /api/hitl/stats
pub async fn stats_handler(State(state): State<HitlApiState>) -> impl IntoResponse {
    let config = state.manager.config();

    let response = StatsResponse {
        pending_count: state.manager.pending_count(),
        enabled: config.enabled,
        confirmation_threshold: config.confirmation_threshold,
    };

    (StatusCode::OK, Json(response))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 解析响应类型
fn parse_response(
    response_type: &str,
    data: Option<serde_json::Value>,
) -> Result<HitlResponse, String> {
    match response_type.to_lowercase().as_str() {
        "approve" => Ok(HitlResponse::Approve),
        "reject" => {
            let reason =
                data.and_then(|d| d.get("reason").and_then(|r| r.as_str().map(String::from)));
            Ok(HitlResponse::Reject { reason })
        }
        "modify" => {
            let modifications = data.ok_or("modifications data is required for modify response")?;
            Ok(HitlResponse::Modify { modifications })
        }
        "abort" => {
            let reason =
                data.and_then(|d| d.get("reason").and_then(|r| r.as_str().map(String::from)));
            Ok(HitlResponse::Abort { reason })
        }
        "clarify" => {
            let selected_option = data.as_ref().and_then(|d| {
                d.get("selected_option")
                    .and_then(|o| o.as_str().map(String::from))
            });
            let input = data
                .as_ref()
                .and_then(|d| d.get("input").and_then(|i| i.as_str().map(String::from)));
            Ok(HitlResponse::Clarify {
                selected_option,
                input,
            })
        }
        "feedback" => {
            let rating = data
                .as_ref()
                .and_then(|d| d.get("rating").and_then(|r| r.as_i64()))
                .map(|n| n as i32);
            let comment = data
                .as_ref()
                .and_then(|d| d.get("comment").and_then(|c| c.as_str().map(String::from)));
            Ok(HitlResponse::ProvideFeedback { rating, comment })
        }
        "resume" => Ok(HitlResponse::Resume),
        _ => Err(format!("Unknown response type: {}", response_type)),
    }
}

// ============================================================================
// Router Builder
// ============================================================================

/// 创建 HITL API 路由
pub fn hitl_router(manager: Arc<HitlManager>) -> axum::Router {
    use axum::routing::{delete, get, post};

    let state = HitlApiState::new(manager);

    axum::Router::new()
        .route("/api/hitl/pending", get(list_pending_handler))
        .route("/api/hitl/stats", get(stats_handler))
        .route("/api/hitl/requests/{id}", get(get_request_handler))
        .route("/api/hitl/requests/{id}/respond", post(respond_handler))
        .route("/api/hitl/requests/{id}", delete(cancel_handler))
        .with_state(state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_response_approve() {
        let response = parse_response("approve", None).unwrap();
        assert!(matches!(response, HitlResponse::Approve));
    }

    #[test]
    fn test_parse_response_reject() {
        let data = serde_json::json!({ "reason": "Not needed" });
        let response = parse_response("reject", Some(data)).unwrap();
        assert!(matches!(response, HitlResponse::Reject { reason: Some(_) }));
    }

    #[test]
    fn test_parse_response_modify() {
        let data = serde_json::json!({ "path": "/tmp/new.txt" });
        let response = parse_response("modify", Some(data)).unwrap();
        assert!(matches!(response, HitlResponse::Modify { .. }));
    }

    #[test]
    fn test_parse_response_clarify() {
        let data = serde_json::json!({ "selected_option": "opt_1", "input": "custom text" });
        let response = parse_response("clarify", Some(data)).unwrap();
        match response {
            HitlResponse::Clarify {
                selected_option,
                input,
            } => {
                assert_eq!(selected_option, Some("opt_1".to_string()));
                assert_eq!(input, Some("custom text".to_string()));
            }
            _ => panic!("Expected Clarify response"),
        }
    }

    #[test]
    fn test_parse_response_feedback() {
        let data = serde_json::json!({
            "rating": 5,
            "comment": "Great job!"
        });
        let response = parse_response("feedback", Some(data)).unwrap();
        match response {
            HitlResponse::ProvideFeedback { rating, comment } => {
                assert_eq!(rating, Some(5));
                assert_eq!(comment, Some("Great job!".to_string()));
            }
            _ => panic!("Expected ProvideFeedback response"),
        }
    }

    #[test]
    fn test_parse_response_resume() {
        let response = parse_response("resume", None).unwrap();
        assert!(matches!(response, HitlResponse::Resume));
    }

    #[test]
    fn test_parse_response_unknown() {
        let result = parse_response("unknown", None);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_response_not_found() {
        let error = ErrorResponse::not_found("test-id");
        assert_eq!(error.code, "NOT_FOUND");
        assert!(error.message.contains("test-id"));
    }
}
