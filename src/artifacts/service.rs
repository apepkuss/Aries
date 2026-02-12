//! Artifact Service Layer
//!
//! Provides business logic for artifact management with SSE event integration.
//! This service wraps the ArtifactStore and emits events during artifact operations.

use std::sync::Arc;

use super::{
    store::{ArtifactResult, ArtifactStore},
    types::*,
};
use crate::chat::emitter::EventEmitter;

/// Artifact service for Agent integration
///
/// This service provides methods that can optionally emit SSE events,
/// making it suitable for use during Agent execution where real-time
/// updates are needed.
#[allow(dead_code)]
pub struct ArtifactService {
    store: Arc<ArtifactStore>,
    base_url: String,
}

#[allow(dead_code)]
impl ArtifactService {
    /// Creates a new ArtifactService
    pub fn new(store: Arc<ArtifactStore>, base_url: impl Into<String>) -> Self {
        Self {
            store,
            base_url: base_url.into(),
        }
    }

    /// Build download URL for an artifact
    fn build_download_url(&self, id: &str) -> String {
        format!("{}/v1/artifacts/{}/download", self.base_url, id)
    }

    /// Creates an artifact and optionally emits SSE event
    ///
    /// # Arguments
    /// * `request` - Artifact creation request
    /// * `user_id` - Optional user ID
    /// * `emitter` - Optional event emitter for SSE notifications
    /// * `subtask_id` - Optional subtask ID for event correlation
    pub async fn create_with_emitter(
        &self,
        request: CreateArtifactRequest,
        user_id: Option<String>,
        emitter: Option<&dyn EventEmitter>,
        subtask_id: Option<usize>,
    ) -> ArtifactResult<Artifact> {
        let content = request.content.clone();
        let artifact_type_json = serde_json::to_value(&request.artifact_type).unwrap_or_default();

        let mut artifact = self.store.create(request, user_id).await?;

        // Set download URL
        artifact.url = Some(self.build_download_url(&artifact.id));

        // Emit SSE event if emitter is provided
        if let Some(emitter) = emitter {
            emitter
                .emit_artifact_created(
                    &artifact.id,
                    &artifact.title,
                    &artifact_type_json,
                    &content,
                    artifact.size,
                    artifact.url.as_deref().unwrap_or(""),
                    subtask_id,
                )
                .await;
        }

        Ok(artifact)
    }

    /// Updates an artifact and optionally emits SSE event
    ///
    /// # Arguments
    /// * `id` - Artifact ID
    /// * `request` - Artifact update request
    /// * `emitter` - Optional event emitter for SSE notifications
    /// * `subtask_id` - Optional subtask ID for event correlation
    pub async fn update_with_emitter(
        &self,
        id: &str,
        request: UpdateArtifactRequest,
        emitter: Option<&dyn EventEmitter>,
        subtask_id: Option<usize>,
    ) -> ArtifactResult<Artifact> {
        let new_content = request.content.clone();

        let artifact = self.store.update(id, request).await?;

        // Emit SSE event if emitter is provided and content was updated
        if let (Some(emitter), Some(ref content)) = (emitter, new_content) {
            emitter
                .emit_artifact_updated(&artifact.id, content, subtask_id)
                .await;
        }

        Ok(artifact)
    }

    /// Deletes an artifact and optionally emits SSE event
    ///
    /// # Arguments
    /// * `id` - Artifact ID
    /// * `emitter` - Optional event emitter for SSE notifications
    /// * `subtask_id` - Optional subtask ID for event correlation
    pub async fn delete_with_emitter(
        &self,
        id: &str,
        emitter: Option<&dyn EventEmitter>,
        subtask_id: Option<usize>,
    ) -> ArtifactResult<()> {
        self.store.delete(id).await?;

        // Emit SSE event if emitter is provided
        if let Some(emitter) = emitter {
            emitter.emit_artifact_deleted(id, subtask_id).await;
        }

        Ok(())
    }

    /// Get underlying store reference
    pub fn store(&self) -> &ArtifactStore {
        &self.store
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;
    use crate::{artifacts::store::ArtifactConfig, chat::emitter::NoopEventEmitter};

    async fn setup_test_service() -> ArtifactService {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test pool");

        let store = ArtifactStore::new(pool, ArtifactConfig::default())
            .await
            .expect("Failed to create test store");

        ArtifactService::new(Arc::new(store), "http://localhost:8080")
    }

    #[tokio::test]
    async fn test_create_with_emitter() {
        let service = setup_test_service().await;
        let emitter = NoopEventEmitter::new();

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.rs".to_string(),
            description: None,
            artifact_type: ArtifactType::Code {
                language: "rust".into(),
            },
            content: "fn main() {}".to_string(),
        };

        let artifact = service
            .create_with_emitter(request, None, Some(&emitter), Some(1))
            .await
            .unwrap();

        assert!(!artifact.id.is_empty());
        assert_eq!(artifact.title, "test.rs");
        assert!(artifact.url.is_some());
        assert!(artifact.url.unwrap().contains("/v1/artifacts/"));
    }

    #[tokio::test]
    async fn test_update_with_emitter() {
        let service = setup_test_service().await;
        let emitter = NoopEventEmitter::new();

        // Create first
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.rs".to_string(),
            description: None,
            artifact_type: ArtifactType::Code {
                language: "rust".into(),
            },
            content: "fn main() {}".to_string(),
        };
        let artifact = service
            .create_with_emitter(request, None, None, None)
            .await
            .unwrap();

        // Update with emitter
        let update_request = UpdateArtifactRequest {
            title: None,
            description: None,
            content: Some("fn main() { println!(\"Hi\"); }".to_string()),
        };

        let updated = service
            .update_with_emitter(&artifact.id, update_request, Some(&emitter), Some(1))
            .await
            .unwrap();

        assert_eq!(updated.size, 29);
    }

    #[tokio::test]
    async fn test_delete_with_emitter() {
        let service = setup_test_service().await;
        let emitter = NoopEventEmitter::new();

        // Create first
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = service
            .create_with_emitter(request, None, None, None)
            .await
            .unwrap();

        // Delete with emitter
        service
            .delete_with_emitter(&artifact.id, Some(&emitter), Some(1))
            .await
            .unwrap();

        // Verify deleted
        let result = service.store().get(&artifact.id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_create_without_emitter() {
        let service = setup_test_service().await;

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };

        // Create without emitter (None)
        let artifact = service
            .create_with_emitter(request, None, None, None)
            .await
            .unwrap();

        assert!(!artifact.id.is_empty());
    }
}
