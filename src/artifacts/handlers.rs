//! HTTP Handlers for Artifacts API
//!
//! Provides RESTful endpoints for artifact management:
//! - POST /v1/artifacts - Create artifact (JSON or multipart)
//! - POST /v1/artifacts/upload - Upload binary artifact (multipart/form-data)
//! - GET /v1/artifacts/{id} - Get artifact details
//! - PUT /v1/artifacts/{id} - Update artifact
//! - DELETE /v1/artifacts/{id} - Delete artifact
//! - GET /v1/artifacts/{id}/download - Download artifact content (supports Range requests)
//! - GET /v1/artifacts/{id}/versions - List versions
//! - GET /v1/artifacts/{id}/versions/{version} - Get specific version content
//! - POST /v1/artifacts/{id}/versions/{version}/restore - Restore to specific version
//! - GET /v1/conversations/{conv_id}/artifacts - List artifacts by conversation

use std::sync::Arc;

use axum::{
    Json,
    body::Body,
    extract::{Multipart, Path, Query, State},
    http::{HeaderMap, Response, StatusCode, header},
};
use once_cell::sync::OnceCell;
use serde::Deserialize;
use sqlx::SqlitePool;

use super::{
    store::{ArtifactConfig, ArtifactError, ArtifactStore},
    types::*,
};
use crate::{dual_error, dual_info};

// ============================================================================
// State
// ============================================================================

/// Artifacts API state
pub struct ArtifactsState {
    store: OnceCell<Arc<ArtifactStore>>,
    db_path: String,
    config: ArtifactConfig,
}

impl ArtifactsState {
    /// Create new state
    pub fn new(db_path: String, config: ArtifactConfig) -> Self {
        Self {
            store: OnceCell::new(),
            db_path,
            config,
        }
    }

    /// Get or create store (lazy initialization)
    pub async fn get_store(&self) -> Result<Arc<ArtifactStore>, ArtifactError> {
        if let Some(store) = self.store.get() {
            return Ok(Arc::clone(store));
        }

        // Create new store
        let connection_string = if self.db_path.starts_with("sqlite:") {
            self.db_path.clone()
        } else {
            format!("sqlite:{}?mode=rwc", self.db_path)
        };

        let pool = SqlitePool::connect(&connection_string)
            .await
            .map_err(|e| ArtifactError::Storage(format!("Database connection failed: {}", e)))?;

        let store = ArtifactStore::new(pool, self.config.clone()).await?;
        let store = Arc::new(store);

        // Store may have been initialized by another request
        let _ = self.store.set(Arc::clone(&store));

        Ok(self.store.get().map(Arc::clone).unwrap_or(store))
    }

    /// Get the configuration
    pub fn config(&self) -> &ArtifactConfig {
        &self.config
    }
}

// ============================================================================
// Query Parameters
// ============================================================================

/// Pagination query parameters
#[derive(Debug, Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub offset: i64,
}

fn default_limit() -> i64 {
    20
}

/// Version query parameter
#[derive(Debug, Deserialize)]
pub struct VersionParams {
    pub version: Option<i32>,
}

/// Binary upload request (from multipart form)
#[derive(Debug)]
#[allow(dead_code)]
struct BinaryUploadRequest {
    conversation_id: String,
    title: String,
    description: Option<String>,
    content: Vec<u8>,
    content_type: Option<String>,
}

// ============================================================================
// Error Response
// ============================================================================

fn error_response(status: StatusCode, message: &str) -> Response<Body> {
    let body = serde_json::json!({
        "error": {
            "message": message,
            "code": status.as_u16()
        }
    });

    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::to_string(&body).unwrap_or_default()))
        .unwrap_or_else(|_| {
            Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(Body::empty())
                .unwrap()
        })
}

fn artifact_error_to_response(err: ArtifactError) -> Response<Body> {
    match &err {
        ArtifactError::NotFound(id) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Artifact not found: {}", id),
        ),
        ArtifactError::VersionNotFound(id, v) => error_response(
            StatusCode::NOT_FOUND,
            &format!("Version {} not found for artifact {}", v, id),
        ),
        ArtifactError::ContentTooLarge(size, max) => error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("Content too large: {} bytes (max: {} bytes)", size, max),
        ),
        _ => error_response(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    }
}

/// Parse HTTP Range header
///
/// Supports format: "bytes=start-end" or "bytes=start-"
/// Returns (start, end) where end is exclusive, or None if invalid
fn parse_range_header(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if !range_header.starts_with("bytes=") {
        return None;
    }

    let range_spec = &range_header[6..]; // Skip "bytes="

    // Handle single range only (not multi-range)
    if range_spec.contains(',') {
        return None;
    }

    let parts: Vec<&str> = range_spec.split('-').collect();
    if parts.len() != 2 {
        return None;
    }

    let start = parts[0].parse::<u64>().ok();
    let end = if parts[1].is_empty() {
        None
    } else {
        parts[1].parse::<u64>().ok()
    };

    match (start, end) {
        // bytes=start-end (inclusive end)
        (Some(s), Some(e)) if s <= e && s < file_size => Some((s, (e + 1).min(file_size))),
        // bytes=start- (from start to end)
        (Some(s), None) if s < file_size => Some((s, file_size)),
        // bytes=-suffix (last N bytes) - not commonly needed, skip for now
        _ => None,
    }
}

// ============================================================================
// Handlers
// ============================================================================

/// POST /v1/artifacts - Create a new artifact
pub async fn create_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Json(request): Json<CreateArtifactRequest>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!(
        "Creating artifact: {} - request_id: {}",
        request.title,
        request_id
    );

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    // Extract user_id from header if present
    let user_id = headers
        .get("x-user-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    match store.create(request, user_id).await {
        Ok(mut artifact) => {
            // Set download URL
            artifact.url = Some(format!("/v1/artifacts/{}/download", artifact.id));

            dual_info!(
                "Created artifact: {} (id={}) - request_id: {}",
                artifact.title,
                artifact.id,
                request_id
            );

            let body = serde_json::to_string(&artifact).unwrap_or_default();
            Response::builder()
                .status(StatusCode::CREATED)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to create artifact: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// GET /v1/artifacts/{id} - Get artifact details
pub async fn get_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!("Getting artifact: {} - request_id: {}", id, request_id);

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.get_with_content(&id).await {
        Ok(Some(mut detail)) => {
            detail.artifact.url = Some(format!("/v1/artifacts/{}/download", id));

            let body = serde_json::to_string(&detail).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Ok(None) => {
            dual_info!("Artifact not found: {} - request_id: {}", id, request_id);
            error_response(
                StatusCode::NOT_FOUND,
                &format!("Artifact not found: {}", id),
            )
        }
        Err(e) => {
            dual_error!("Failed to get artifact: {} - request_id: {}", e, request_id);
            artifact_error_to_response(e)
        }
    }
}

/// PUT /v1/artifacts/{id} - Update artifact
pub async fn update_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(request): Json<UpdateArtifactRequest>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!("Updating artifact: {} - request_id: {}", id, request_id);

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.update(&id, request).await {
        Ok(mut artifact) => {
            artifact.url = Some(format!("/v1/artifacts/{}/download", id));

            dual_info!(
                "Updated artifact: {} (version={}) - request_id: {}",
                artifact.title,
                artifact.version,
                request_id
            );

            let body = serde_json::to_string(&artifact).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to update artifact: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// DELETE /v1/artifacts/{id} - Delete artifact
pub async fn delete_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!("Deleting artifact: {} - request_id: {}", id, request_id);

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.delete(&id).await {
        Ok(()) => {
            dual_info!("Deleted artifact: {} - request_id: {}", id, request_id);
            Response::builder()
                .status(StatusCode::NO_CONTENT)
                .body(Body::empty())
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to delete artifact: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// GET /v1/artifacts/{id}/download - Download artifact content
///
/// Supports HTTP Range requests for partial content (useful for large files/streaming).
/// Returns 206 Partial Content for Range requests, 200 OK otherwise.
pub async fn download_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(params): Query<VersionParams>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!("Downloading artifact: {} - request_id: {}", id, request_id);

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    // Get artifact metadata
    let artifact = match store.get(&id).await {
        Ok(Some(a)) => a,
        Ok(None) => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!("Artifact not found: {}", id),
            );
        }
        Err(e) => {
            dual_error!("Failed to get artifact: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    let version = params.version.unwrap_or(artifact.version);
    let mime_type = artifact.artifact_type.mime_type();
    let filename = &artifact.title;
    let file_size = artifact.size;

    // Check for Range header
    let range_header = headers.get(header::RANGE).and_then(|h| h.to_str().ok());

    if let Some(range_str) = range_header {
        // Handle Range request
        if let Some((start, end)) = parse_range_header(range_str, file_size) {
            let length = end - start;

            match store
                .get_content_range(&id, version, start, Some(length))
                .await
            {
                Ok(content) => {
                    dual_info!(
                        "Serving range {}-{}/{} for artifact: {} - request_id: {}",
                        start,
                        end - 1,
                        file_size,
                        id,
                        request_id
                    );

                    Response::builder()
                        .status(StatusCode::PARTIAL_CONTENT)
                        .header(header::CONTENT_TYPE, mime_type)
                        .header(
                            header::CONTENT_DISPOSITION,
                            format!("attachment; filename=\"{}\"", filename),
                        )
                        .header(header::CONTENT_LENGTH, content.len())
                        .header(
                            header::CONTENT_RANGE,
                            format!("bytes {}-{}/{}", start, end - 1, file_size),
                        )
                        .header(header::ACCEPT_RANGES, "bytes")
                        .body(Body::from(content))
                        .unwrap()
                }
                Err(e) => {
                    dual_error!(
                        "Failed to read range for artifact: {} - request_id: {}",
                        e,
                        request_id
                    );
                    artifact_error_to_response(e)
                }
            }
        } else {
            // Invalid range
            Response::builder()
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{}", file_size))
                .body(Body::empty())
                .unwrap()
        }
    } else {
        // Full content request
        match store.get_content(&id, version).await {
            Ok(content) => Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime_type)
                .header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", filename),
                )
                .header(header::CONTENT_LENGTH, content.len())
                .header(header::ACCEPT_RANGES, "bytes")
                .body(Body::from(content))
                .unwrap(),
            Err(e) => {
                dual_error!(
                    "Failed to download artifact: {} - request_id: {}",
                    e,
                    request_id
                );
                artifact_error_to_response(e)
            }
        }
    }
}

/// GET /v1/artifacts/{id}/versions - List artifact versions
pub async fn list_versions_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!(
        "Listing versions for artifact: {} - request_id: {}",
        id,
        request_id
    );

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.get_versions(&id).await {
        Ok(versions) => {
            let response = ArtifactVersionListResponse {
                total: versions.len(),
                versions,
            };

            let body = serde_json::to_string(&response).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to list versions: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// GET /v1/artifacts/{id}/versions/{version} - Get specific version content
pub async fn get_version_content_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, i32)>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!(
        "Getting version {} content for artifact: {} - request_id: {}",
        version,
        id,
        request_id
    );

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.get_version_content(&id, version).await {
        Ok(mut detail) => {
            detail.artifact.url =
                Some(format!("/v1/artifacts/{}/download?version={}", id, version));

            let body = serde_json::to_string(&detail).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to get version {} for artifact {}: {} - request_id: {}",
                version,
                id,
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// POST /v1/artifacts/{id}/versions/{version}/restore - Restore to specific version
pub async fn restore_version_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path((id, version)): Path<(String, i32)>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!(
        "Restoring artifact {} to version {} - request_id: {}",
        id,
        version,
        request_id
    );

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store.restore_version(&id, version).await {
        Ok(mut artifact) => {
            artifact.url = Some(format!("/v1/artifacts/{}/download", id));

            dual_info!(
                "Restored artifact {} to version {} (new version={}) - request_id: {}",
                id,
                version,
                artifact.version,
                request_id
            );

            let body = serde_json::to_string(&artifact).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to restore artifact {} to version {}: {} - request_id: {}",
                id,
                version,
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// GET /v1/conversations/{conv_id}/artifacts - List artifacts by conversation
pub async fn list_artifacts_by_conversation_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    Path(conv_id): Path<String>,
    Query(params): Query<PaginationParams>,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!(
        "Listing artifacts for conversation: {} - request_id: {}",
        conv_id,
        request_id
    );

    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    match store
        .list_by_conversation(&conv_id, params.limit, params.offset)
        .await
    {
        Ok((mut artifacts, total)) => {
            // Set download URLs
            for artifact in &mut artifacts {
                artifact.url = Some(format!("/v1/artifacts/{}/download", artifact.id));
            }

            let has_more = (params.offset + params.limit) < total;
            let response = ArtifactListResponse {
                artifacts,
                total: total as usize,
                has_more,
            };

            let body = serde_json::to_string(&response).unwrap_or_default();
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to list artifacts: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

// ============================================================================
// Binary Upload Handler (Phase 6)
// ============================================================================

/// POST /v1/artifacts/upload - Upload binary artifact via multipart/form-data
///
/// Expects multipart form with fields:
/// - `conversation_id`: Required conversation ID
/// - `title`: Required file title
/// - `description`: Optional description
/// - `file`: Required file content
pub async fn upload_binary_artifact_handler(
    State(state): State<Arc<ArtifactsState>>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response<Body> {
    let request_id = headers
        .get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("unknown");

    dual_info!("Uploading binary artifact - request_id: {}", request_id);

    // Parse multipart form
    let mut conversation_id: Option<String> = None;
    let mut title: Option<String> = None;
    let mut description: Option<String> = None;
    let mut content: Option<Vec<u8>> = None;
    let mut content_type: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let field_name = field.name().map(|s| s.to_string());

        match field_name.as_deref() {
            Some("conversation_id") => {
                if let Ok(text) = field.text().await {
                    conversation_id = Some(text);
                }
            }
            Some("title") => {
                if let Ok(text) = field.text().await {
                    title = Some(text);
                }
            }
            Some("description") => {
                if let Ok(text) = field.text().await
                    && !text.is_empty()
                {
                    description = Some(text);
                }
            }
            Some("file") => {
                // Get content type from the field
                content_type = field.content_type().map(|s| s.to_string());

                // Use filename as title if not provided
                if title.is_none()
                    && let Some(filename) = field.file_name()
                {
                    title = Some(filename.to_string());
                }

                // Read file content
                match field.bytes().await {
                    Ok(bytes) => {
                        content = Some(bytes.to_vec());
                    }
                    Err(e) => {
                        dual_error!(
                            "Failed to read file content: {} - request_id: {}",
                            e,
                            request_id
                        );
                        return error_response(
                            StatusCode::BAD_REQUEST,
                            &format!("Failed to read file content: {}", e),
                        );
                    }
                }
            }
            _ => {
                // Ignore unknown fields
            }
        }
    }

    // Validate required fields
    let conversation_id = match conversation_id {
        Some(id) => id,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Missing required field: conversation_id",
            );
        }
    };

    let title = match title {
        Some(t) => t,
        None => {
            return error_response(
                StatusCode::BAD_REQUEST,
                "Missing required field: title or file",
            );
        }
    };

    let content = match content {
        Some(c) => c,
        None => {
            return error_response(StatusCode::BAD_REQUEST, "Missing required field: file");
        }
    };

    // Determine artifact type from content type or file extension
    let artifact_type = if let Some(ref ct) = content_type {
        artifact_type_from_mime(ct)
    } else {
        // Try to infer from filename extension
        let ext = title.rsplit('.').next().unwrap_or("");
        ArtifactType::from_extension(ext)
    };

    // Get store
    let store = match state.get_store().await {
        Ok(s) => s,
        Err(e) => {
            dual_error!("Failed to get store: {} - request_id: {}", e, request_id);
            return artifact_error_to_response(e);
        }
    };

    // Check size limit
    let max_size = if artifact_type.is_binary() {
        state.config().max_binary_size
    } else {
        state.config().max_content_size
    };

    if content.len() as u64 > max_size {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!(
                "File too large: {} bytes (max: {} bytes)",
                content.len(),
                max_size
            ),
        );
    }

    // Extract user_id from header
    let user_id = headers
        .get("x-user-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Create artifact using binary content
    match store
        .create_binary(
            conversation_id,
            title.clone(),
            description,
            artifact_type,
            content,
            user_id,
        )
        .await
    {
        Ok(mut artifact) => {
            artifact.url = Some(format!("/v1/artifacts/{}/download", artifact.id));

            dual_info!(
                "Uploaded binary artifact: {} (id={}, size={}) - request_id: {}",
                artifact.title,
                artifact.id,
                artifact.size,
                request_id
            );

            let body = serde_json::to_string(&artifact).unwrap_or_default();
            Response::builder()
                .status(StatusCode::CREATED)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap()
        }
        Err(e) => {
            dual_error!(
                "Failed to create binary artifact: {} - request_id: {}",
                e,
                request_id
            );
            artifact_error_to_response(e)
        }
    }
}

/// Infer artifact type from MIME type
fn artifact_type_from_mime(mime_type: &str) -> ArtifactType {
    match mime_type {
        // Images
        "image/png" => ArtifactType::Image {
            format: "png".into(),
        },
        "image/jpeg" => ArtifactType::Image {
            format: "jpeg".into(),
        },
        "image/gif" => ArtifactType::Image {
            format: "gif".into(),
        },
        "image/webp" => ArtifactType::Image {
            format: "webp".into(),
        },
        "image/svg+xml" => ArtifactType::Svg,
        "image/x-icon" | "image/vnd.microsoft.icon" => ArtifactType::Image {
            format: "ico".into(),
        },
        "image/bmp" => ArtifactType::Image {
            format: "bmp".into(),
        },

        // PDF
        "application/pdf" => ArtifactType::Pdf,

        // Audio
        "audio/mpeg" | "audio/mp3" => ArtifactType::Audio {
            format: "mp3".into(),
        },
        "audio/wav" | "audio/x-wav" => ArtifactType::Audio {
            format: "wav".into(),
        },
        "audio/ogg" => ArtifactType::Audio {
            format: "ogg".into(),
        },
        "audio/flac" => ArtifactType::Audio {
            format: "flac".into(),
        },
        "audio/aac" => ArtifactType::Audio {
            format: "aac".into(),
        },
        "audio/webm" => ArtifactType::Audio {
            format: "webm".into(),
        },

        // Video
        "video/mp4" => ArtifactType::Video {
            format: "mp4".into(),
        },
        "video/webm" => ArtifactType::Video {
            format: "webm".into(),
        },
        "video/ogg" => ArtifactType::Video {
            format: "ogg".into(),
        },
        "video/x-msvideo" => ArtifactType::Video {
            format: "avi".into(),
        },
        "video/quicktime" => ArtifactType::Video {
            format: "mov".into(),
        },

        // Text types
        "text/html" => ArtifactType::Html,
        "text/markdown" => ArtifactType::Markdown,
        "application/json" => ArtifactType::Json,
        "text/plain" => ArtifactType::Text,

        // Default: generic binary
        _ => ArtifactType::Binary {
            mime_type: mime_type.to_string(),
        },
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pagination_params_defaults() {
        let params: PaginationParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.limit, 20);
        assert_eq!(params.offset, 0);
    }

    #[test]
    fn test_pagination_params_custom() {
        let params: PaginationParams =
            serde_json::from_str(r#"{"limit": 50, "offset": 100}"#).unwrap();
        assert_eq!(params.limit, 50);
        assert_eq!(params.offset, 100);
    }

    #[test]
    fn test_version_params() {
        let params: VersionParams = serde_json::from_str(r#"{"version": 3}"#).unwrap();
        assert_eq!(params.version, Some(3));

        let params: VersionParams = serde_json::from_str("{}").unwrap();
        assert_eq!(params.version, None);
    }

    #[test]
    fn test_error_response_format() {
        let response = error_response(StatusCode::NOT_FOUND, "Test error");
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
