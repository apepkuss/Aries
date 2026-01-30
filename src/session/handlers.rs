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
