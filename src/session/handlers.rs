use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};

use super::{
    reader::SessionReader,
    types::{SessionError, SessionMeta, SessionRecord},
};
use crate::AppState;

// ============================================================================
// Request / Response types
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct ListSessionsParams {
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct ListSessionsResponse {
    pub sessions: Vec<SessionMeta>,
    pub total: usize,
}

#[derive(Debug, Deserialize)]
pub struct GetSessionParams {
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct GetSessionResponse {
    pub records: Vec<SessionRecord>,
}

#[derive(Debug, Deserialize)]
pub struct DeleteSessionParams {
    pub user_id: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteSessionResponse {
    pub success: bool,
    pub session_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct BatchDeleteSessionsRequest {
    pub user_id: String,
    /// If provided and non-empty, delete only these sessions.
    /// If absent or empty, delete ALL sessions for the user.
    #[serde(default)]
    pub session_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchDeleteSessionsResponse {
    pub success: bool,
    pub deleted_count: usize,
    pub deleted_ids: Vec<String>,
    pub failed_ids: Vec<String>,
    pub message: String,
}

// ============================================================================
// Handlers
// ============================================================================

/// GET /v1/sessions?user_id=xxx
///
/// List all sessions for the given user.
pub async fn list_sessions_handler(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListSessionsParams>,
) -> Result<Json<ListSessionsResponse>, SessionApiError> {
    let reader = get_reader(&state)?;
    let sessions = reader.list_sessions(&params.user_id).await?;
    let total = sessions.len();

    Ok(Json(ListSessionsResponse { sessions, total }))
}

/// GET /v1/sessions/:id?user_id=xxx
///
/// Get all records from a specific session.
pub async fn get_session_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(params): Query<GetSessionParams>,
) -> Result<Json<GetSessionResponse>, SessionApiError> {
    let reader = get_reader(&state)?;
    let records = reader.read_session(&params.user_id, &session_id).await?;

    Ok(Json(GetSessionResponse { records }))
}

/// DELETE /v1/sessions/:id?user_id=xxx
///
/// Delete a specific session.
pub async fn delete_session_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Query(params): Query<DeleteSessionParams>,
) -> Result<Json<DeleteSessionResponse>, SessionApiError> {
    let reader = get_reader(&state)?;
    reader.delete_session(&params.user_id, &session_id).await?;

    Ok(Json(DeleteSessionResponse {
        success: true,
        session_id,
        message: "Session deleted successfully".to_string(),
    }))
}

/// POST /v1/sessions/batch-delete
///
/// Batch delete sessions.
/// - If `session_ids` is non-empty, delete only those sessions.
/// - If `session_ids` is empty, delete ALL sessions for the user.
pub async fn batch_delete_sessions_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<BatchDeleteSessionsRequest>,
) -> Result<Json<BatchDeleteSessionsResponse>, SessionApiError> {
    let reader = get_reader(&state)?;

    if body.session_ids.is_empty() {
        // Delete all sessions
        let count = reader.delete_all_sessions(&body.user_id).await?;
        Ok(Json(BatchDeleteSessionsResponse {
            success: true,
            deleted_count: count,
            deleted_ids: vec![],
            failed_ids: vec![],
            message: format!("Deleted all sessions ({count})"),
        }))
    } else {
        // Batch delete specified sessions
        let result = reader
            .delete_sessions(&body.user_id, &body.session_ids)
            .await;
        let count = result.deleted.len();
        Ok(Json(BatchDeleteSessionsResponse {
            success: result.failed.is_empty(),
            deleted_count: count,
            deleted_ids: result.deleted,
            failed_ids: result.failed,
            message: format!("Deleted {count} session(s)"),
        }))
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Get a SessionReader from AppState.
///
/// Returns an error if the session feature is not enabled.
fn get_reader(state: &AppState) -> Result<SessionReader, SessionApiError> {
    let writer = state
        .session_writer
        .as_ref()
        .ok_or(SessionApiError::Disabled)?;

    // Reader uses the same base_dir as writer
    Ok(SessionReader::new(writer.base_dir()))
}

// ============================================================================
// Error type
// ============================================================================

/// API error type for session endpoints.
///
/// Converts `SessionError` into appropriate HTTP responses.
#[derive(Debug)]
pub enum SessionApiError {
    Disabled,
    Session(SessionError),
}

impl From<SessionError> for SessionApiError {
    fn from(err: SessionError) -> Self {
        SessionApiError::Session(err)
    }
}

impl IntoResponse for SessionApiError {
    fn into_response(self) -> axum::response::Response {
        match self {
            SessionApiError::Disabled => {
                let body = serde_json::json!({
                    "error": {
                        "message": "Session feature is not enabled",
                        "type": "service_unavailable",
                        "code": "session_disabled"
                    }
                });
                (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
            }
            SessionApiError::Session(SessionError::NotFound(id)) => {
                let body = serde_json::json!({
                    "error": {
                        "message": format!("Session not found: {id}"),
                        "type": "not_found",
                        "code": "session_not_found"
                    }
                });
                (StatusCode::NOT_FOUND, Json(body)).into_response()
            }
            SessionApiError::Session(e) => {
                let body = serde_json::json!({
                    "error": {
                        "message": format!("Session error: {e}"),
                        "type": "internal_error",
                        "code": "session_error"
                    }
                });
                (StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
            }
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use axum::{http::StatusCode, response::IntoResponse};

    use super::*;
    use crate::session::types::SessionError;

    #[test]
    fn test_session_api_error_disabled_returns_503() {
        let err = SessionApiError::Disabled;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn test_session_api_error_not_found_returns_404() {
        let err = SessionApiError::Session(SessionError::NotFound("test-id".to_string()));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn test_session_api_error_io_returns_500() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let err = SessionApiError::Session(SessionError::Io(io_err));
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_session_api_error_from_session_error() {
        let session_err = SessionError::NotFound("abc".to_string());
        let api_err: SessionApiError = session_err.into();
        assert!(matches!(
            api_err,
            SessionApiError::Session(SessionError::NotFound(_))
        ));
    }

    #[test]
    fn test_get_reader_returns_disabled_when_no_writer() {
        let state = AppState::new(
            crate::config::Config::default(),
            crate::info::ServerInfo::default(),
        );
        let result = get_reader(&state);
        assert!(matches!(result, Err(SessionApiError::Disabled)));
    }

    #[tokio::test]
    async fn test_get_reader_returns_reader_when_writer_present() {
        let tmp = tempfile::TempDir::new().unwrap();
        let writer = crate::session::writer::SessionWriter::new(tmp.path());
        let state = AppState::new(
            crate::config::Config::default(),
            crate::info::ServerInfo::default(),
        )
        .with_session_writer(writer);

        let result = get_reader(&state);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_list_sessions_response_format() {
        let tmp = tempfile::TempDir::new().unwrap();
        let writer = crate::session::writer::SessionWriter::new(tmp.path());

        // Write a session
        let sid = writer.get_or_create_session_id("test_user").await;
        let seq = writer.next_sequence("test_user").await;
        writer
            .append_message(
                "test_user",
                &sid,
                "gpt-4",
                super::super::types::SessionRecord::Message {
                    version: super::super::types::JSONL_FORMAT_VERSION,
                    role: "user".to_string(),
                    content: "Hello".to_string(),
                    timestamp: chrono::Utc::now(),
                    message_id: "msg_1".to_string(),
                    sequence: seq,
                    tokens: None,
                    tool_calls: None,
                    privacy_mode: false,
                },
            )
            .await
            .unwrap();

        let reader = super::super::reader::SessionReader::new(tmp.path());
        let sessions = reader.list_sessions("test_user").await.unwrap();
        let total = sessions.len();

        let response = ListSessionsResponse { sessions, total };
        let json = serde_json::to_value(&response).unwrap();

        // Verify response structure
        assert!(json["sessions"].is_array());
        assert_eq!(json["total"], 1);
        assert_eq!(json["sessions"][0]["model"], "gpt-4");
        assert_eq!(json["sessions"][0]["message_count"], 1);
    }

    #[test]
    fn test_delete_response_serialization() {
        let response = DeleteSessionResponse {
            success: true,
            session_id: "test-session".to_string(),
            message: "Session deleted successfully".to_string(),
        };
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["session_id"], "test-session");
        assert!(json["message"].as_str().unwrap().contains("deleted"));
    }
}
