//! Sub-Agent HTTP API 处理器
//!
//! 此模块实现 Sub-Agent 的 HTTP API 端点，用于查询和管理 Sub-Agent。
//!
//! # 端点
//!
//! - `GET /api/subagents` - 列出所有 Sub-Agent
//! - `GET /api/subagents/{id}` - 获取指定 Sub-Agent 详情
//! - `POST /api/subagents/{id}/cancel` - 取消指定 Sub-Agent
//! - `GET /api/subagents/stats` - 获取 Sub-Agent 统计信息

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::{SubAgent, SubAgentId, SubAgentState, manager::SubAgentManager};

// ============================================================================
// Request/Response Types
// ============================================================================

/// 列表查询参数
#[derive(Debug, Deserialize)]
pub struct ListSubAgentsQuery {
    /// 按状态筛选
    #[serde(default)]
    pub state: Option<String>,
    /// 按父 ID 筛选
    #[serde(default)]
    pub parent_id: Option<String>,
    /// 限制返回数量
    #[serde(default)]
    pub limit: Option<usize>,
    /// 偏移量
    #[serde(default)]
    pub offset: Option<usize>,
}

/// Sub-Agent 列表响应
#[derive(Debug, Serialize)]
pub struct ListSubAgentsResponse {
    /// Sub-Agent 列表
    pub subagents: Vec<SubAgentSummary>,
    /// 总数
    pub total: usize,
    /// 是否有更多
    pub has_more: bool,
}

/// Sub-Agent 摘要信息（用于列表展示）
#[derive(Debug, Serialize)]
pub struct SubAgentSummary {
    /// ID
    pub id: String,
    /// 名称
    pub name: String,
    /// 状态
    pub state: SubAgentState,
    /// 任务描述（截断）
    pub task: String,
    /// 嵌套深度
    pub depth: u32,
    /// 父 ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    /// 创建时间
    pub created_at: u64,
    /// 执行时长（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

impl From<&SubAgent> for SubAgentSummary {
    fn from(agent: &SubAgent) -> Self {
        let task = if agent.task.len() > 100 {
            format!("{}...", &agent.task[..100])
        } else {
            agent.task.clone()
        };

        Self {
            id: agent.id.to_string(),
            name: agent.name.clone(),
            state: agent.state,
            task,
            depth: agent.depth,
            parent_id: agent.parent_id.as_ref().map(|id| id.to_string()),
            created_at: agent.created_at,
            duration_ms: agent.duration().map(|d| d.as_millis() as u64),
        }
    }
}

/// Sub-Agent 详情响应
#[derive(Debug, Serialize)]
pub struct SubAgentDetailResponse {
    /// Sub-Agent 完整信息
    #[serde(flatten)]
    pub subagent: SubAgent,
}

/// 取消响应
#[derive(Debug, Serialize)]
pub struct CancelResponse {
    /// 是否成功
    pub success: bool,
    /// 消息
    pub message: String,
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
            message: format!("Sub-Agent '{}' not found", id),
        }
    }

    pub fn invalid_state(msg: impl Into<String>) -> Self {
        Self {
            code: "INVALID_STATE".to_string(),
            message: msg.into(),
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

/// Sub-Agent API 状态
///
/// 包含 SubAgentManager 引用
#[derive(Clone)]
pub struct SubAgentApiState {
    pub manager: Arc<SubAgentManager>,
}

impl SubAgentApiState {
    pub fn new(manager: Arc<SubAgentManager>) -> Self {
        Self { manager }
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// 列出所有 Sub-Agent
///
/// GET /api/subagents
///
/// 支持的查询参数：
/// - `state`: 按状态筛选（pending, running, completed, failed, cancelled）
/// - `parent_id`: 按父 ID 筛选
/// - `limit`: 限制返回数量（默认 50）
/// - `offset`: 分页偏移量（默认 0）
pub async fn list_subagents_handler(
    State(state): State<SubAgentApiState>,
    Query(query): Query<ListSubAgentsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    // 获取 Sub-Agent 列表
    let agents = if let Some(state_filter) = &query.state {
        // 按状态筛选
        match parse_state(state_filter) {
            Some(filter_state) => state.manager.list_by_state(filter_state).await,
            None => {
                // 无效状态，返回空列表
                Vec::new()
            }
        }
    } else if let Some(parent_id) = &query.parent_id {
        // 按父 ID 筛选
        let parent = SubAgentId::from_string(parent_id.clone());
        state.manager.list_children(&parent).await
    } else {
        // 获取所有
        state.manager.list().await
    };

    let total = agents.len();

    // 应用分页
    let paginated: Vec<SubAgentSummary> = agents
        .iter()
        .skip(offset)
        .take(limit)
        .map(SubAgentSummary::from)
        .collect();

    let has_more = offset + paginated.len() < total;

    let response = ListSubAgentsResponse {
        subagents: paginated,
        total,
        has_more,
    };

    (StatusCode::OK, Json(response))
}

/// 获取 Sub-Agent 详情
///
/// GET /api/subagents/{id}
pub async fn get_subagent_handler(
    State(state): State<SubAgentApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let subagent_id = SubAgentId::from_string(&id);

    match state.manager.get(&subagent_id).await {
        Ok(agent) => {
            let response = SubAgentDetailResponse { subagent: agent };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(_) => {
            let error = ErrorResponse::not_found(&id);
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::to_value(error).unwrap()),
            )
                .into_response()
        }
    }
}

/// 取消 Sub-Agent
///
/// POST /api/subagents/{id}/cancel
pub async fn cancel_subagent_handler(
    State(state): State<SubAgentApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let subagent_id = SubAgentId::from_string(&id);

    match state.manager.cancel(&subagent_id).await {
        Ok(()) => {
            let response = CancelResponse {
                success: true,
                message: format!("Sub-Agent '{}' has been cancelled", id),
            };
            (
                StatusCode::OK,
                Json(serde_json::to_value(response).unwrap()),
            )
                .into_response()
        }
        Err(e) => {
            let (status, error) = match &e {
                crate::error::ServerError::SubAgentNotFound(_) => {
                    (StatusCode::NOT_FOUND, ErrorResponse::not_found(&id))
                }
                crate::error::ServerError::SubAgentAlreadyTerminal { state, .. } => (
                    StatusCode::CONFLICT,
                    ErrorResponse::invalid_state(format!(
                        "Sub-Agent is already in terminal state: {}",
                        state
                    )),
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

/// 获取 Sub-Agent 统计信息
///
/// GET /api/subagents/stats
pub async fn get_subagent_stats_handler(
    State(state): State<SubAgentApiState>,
) -> impl IntoResponse {
    let stats = state.manager.stats().await;
    (StatusCode::OK, Json(stats))
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 解析状态字符串
fn parse_state(s: &str) -> Option<SubAgentState> {
    match s.to_lowercase().as_str() {
        "pending" => Some(SubAgentState::Pending),
        "running" => Some(SubAgentState::Running),
        "completed" => Some(SubAgentState::Completed),
        "failed" => Some(SubAgentState::Failed),
        "cancelled" => Some(SubAgentState::Cancelled),
        _ => None,
    }
}

// ============================================================================
// Router Builder
// ============================================================================

/// 创建 Sub-Agent API 路由
///
/// 返回配置好的 axum Router
pub fn subagent_router(manager: Arc<SubAgentManager>) -> axum::Router {
    use axum::routing::{get, post};

    let state = SubAgentApiState::new(manager);

    axum::Router::new()
        .route("/api/subagents", get(list_subagents_handler))
        .route("/api/subagents/stats", get(get_subagent_stats_handler))
        .route("/api/subagents/{id}", get(get_subagent_handler))
        .route("/api/subagents/{id}/cancel", post(cancel_subagent_handler))
        .with_state(state)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::SubAgentSystemConfig;

    fn create_test_manager() -> Arc<SubAgentManager> {
        Arc::new(SubAgentManager::new(SubAgentSystemConfig::default_enabled()))
    }

    #[test]
    fn test_parse_state() {
        assert_eq!(parse_state("pending"), Some(SubAgentState::Pending));
        assert_eq!(parse_state("RUNNING"), Some(SubAgentState::Running));
        assert_eq!(parse_state("Completed"), Some(SubAgentState::Completed));
        assert_eq!(parse_state("failed"), Some(SubAgentState::Failed));
        assert_eq!(parse_state("cancelled"), Some(SubAgentState::Cancelled));
        assert_eq!(parse_state("invalid"), None);
    }

    #[test]
    fn test_subagent_summary_from_agent() {
        let agent = SubAgent::new(
            "TestAgent",
            "System prompt",
            "A very long task description that should be truncated when converted to summary",
        );
        let summary = SubAgentSummary::from(&agent);

        assert_eq!(summary.name, "TestAgent");
        assert_eq!(summary.state, SubAgentState::Pending);
        assert_eq!(summary.depth, 0);
        assert!(summary.parent_id.is_none());
    }

    #[test]
    fn test_subagent_summary_truncates_task() {
        let long_task = "a".repeat(200);
        let agent = SubAgent::new("Agent", "System", &long_task);
        let summary = SubAgentSummary::from(&agent);

        assert!(summary.task.len() < 200);
        assert!(summary.task.ends_with("..."));
    }

    #[test]
    fn test_error_response() {
        let not_found = ErrorResponse::not_found("sa-123");
        assert_eq!(not_found.code, "NOT_FOUND");
        assert!(not_found.message.contains("sa-123"));

        let invalid_state = ErrorResponse::invalid_state("Already completed");
        assert_eq!(invalid_state.code, "INVALID_STATE");
    }

    #[test]
    fn test_list_response_serialization() {
        let response = ListSubAgentsResponse {
            subagents: vec![],
            total: 0,
            has_more: false,
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"total\":0"));
        assert!(json.contains("\"has_more\":false"));
    }

    #[test]
    fn test_cancel_response_serialization() {
        let response = CancelResponse {
            success: true,
            message: "Cancelled".to_string(),
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"success\":true"));
    }

    #[tokio::test]
    async fn test_api_state_creation() {
        let manager = create_test_manager();
        let state = SubAgentApiState::new(Arc::clone(&manager));

        // Verify manager is accessible
        assert!(state.manager.is_enabled());
    }

    #[tokio::test]
    async fn test_manager_integration() {
        let manager = create_test_manager();

        // Spawn some agents
        let id1 = manager
            .spawn("Agent1", "system", "task1", None, None)
            .await
            .unwrap();
        let id2 = manager
            .spawn("Agent2", "system", "task2", None, None)
            .await
            .unwrap();

        // Verify list works
        let agents = manager.list().await;
        assert_eq!(agents.len(), 2);

        // Verify state filter works
        manager.mark_started(&id1).await.unwrap();
        let running = manager.list_by_state(SubAgentState::Running).await;
        assert_eq!(running.len(), 1);

        // Verify stats works
        let stats = manager.stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.running, 1);
        assert_eq!(stats.pending, 1);

        // Verify cancel works
        manager.cancel(&id2).await.unwrap();
        let agent2 = manager.get(&id2).await.unwrap();
        assert_eq!(agent2.state, SubAgentState::Cancelled);
    }

    #[tokio::test]
    async fn test_router_creation() {
        let manager = create_test_manager();
        let router = subagent_router(manager);

        // Just verify router can be created without panic
        // Actual route testing requires axum-test or similar
        let _ = router;
    }

    #[test]
    fn test_summary_with_parent() {
        let parent_id = SubAgentId::from_string("sa-parent");
        let agent = SubAgent::new("Child", "System", "Task").with_parent(parent_id.clone(), 0);

        let summary = SubAgentSummary::from(&agent);

        assert_eq!(summary.depth, 1);
        assert_eq!(summary.parent_id, Some("sa-parent".to_string()));
    }

    #[test]
    fn test_error_response_variants() {
        let not_found = ErrorResponse::not_found("test-id");
        assert_eq!(not_found.code, "NOT_FOUND");

        let invalid_state = ErrorResponse::invalid_state("bad state");
        assert_eq!(invalid_state.code, "INVALID_STATE");

        let internal = ErrorResponse::internal_error("something broke");
        assert_eq!(internal.code, "INTERNAL_ERROR");
    }
}
