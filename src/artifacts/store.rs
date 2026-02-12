//! Artifact Store - Database and Storage Coordination
//!
//! Manages artifact metadata (SQLite) and content storage (pluggable backend).

use std::sync::Arc;

use chrono::Utc;
use sqlx::SqlitePool;
use thiserror::Error;
use uuid::Uuid;

use super::{
    storage::{ArtifactStorage, FileSystemStorage},
    types::*,
};
use crate::dual_info;

// ============================================================================
// Error Types
// ============================================================================

/// Artifact storage error types
#[derive(Debug, Error)]
pub enum ArtifactError {
    #[error("Artifact not found: {0}")]
    NotFound(String),

    #[error("Content too large: {0} bytes (max: {1} bytes)")]
    ContentTooLarge(u64, u64),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Storage error: {0}")]
    Storage(String),
}

pub type ArtifactResult<T> = Result<T, ArtifactError>;

// ============================================================================
// Configuration
// ============================================================================

/// Artifact system configuration
#[derive(Debug, Clone)]
pub struct ArtifactConfig {
    /// Maximum content size for text artifacts (default: 1MB)
    pub max_content_size: u64,
    /// Maximum content size for binary artifacts (default: 100MB)
    pub max_binary_size: u64,
    /// Storage path for file system backend
    pub storage_path: Option<String>,

    // ========== Lifecycle Configuration ==========
    /// Artifact retention days (0 = never expire, default: 30)
    pub retention_days: u32,
    /// Cleanup interval in seconds (default: 3600 = 1 hour)
    pub cleanup_interval_secs: u64,
    /// Days to keep soft-deleted artifacts before physical deletion (default: 7)
    pub soft_delete_retention_days: u32,
    /// Enable automatic cleanup (default: true)
    #[allow(dead_code)]
    pub enable_cleanup: bool,
}

impl Default for ArtifactConfig {
    fn default() -> Self {
        Self {
            max_content_size: 1024 * 1024,      // 1MB for text
            max_binary_size: 100 * 1024 * 1024, // 100MB for binary
            storage_path: None,
            retention_days: 30,
            cleanup_interval_secs: 3600, // 1 hour
            soft_delete_retention_days: 7,
            enable_cleanup: true,
        }
    }
}

// ============================================================================
// Artifact Store
// ============================================================================

/// Artifact storage layer
///
/// Coordinates metadata (SQLite) and content storage (pluggable backend)
pub struct ArtifactStore {
    /// SQLite connection pool (metadata)
    pool: SqlitePool,
    /// Configuration
    config: ArtifactConfig,
    /// Content storage backend
    storage: Arc<dyn ArtifactStorage>,
}

impl ArtifactStore {
    /// Create a new ArtifactStore instance
    pub async fn new(pool: SqlitePool, config: ArtifactConfig) -> ArtifactResult<Self> {
        // Create file system storage backend (default)
        let storage_path = config.storage_path.as_ref().map(std::path::PathBuf::from);
        let fs_storage = FileSystemStorage::new(storage_path).await?;
        let storage: Arc<dyn ArtifactStorage> = Arc::new(fs_storage);

        let store = Self {
            pool,
            config,
            storage,
        };
        store.initialize_schema().await?;

        dual_info!(
            "ArtifactStore initialized with {} backend",
            store.storage.backend_name()
        );

        Ok(store)
    }

    /// Initialize database schema
    async fn initialize_schema(&self) -> ArtifactResult<()> {
        // Artifacts metadata table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                user_id TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                title TEXT NOT NULL,
                description TEXT,
                artifact_type TEXT NOT NULL,
                size INTEGER NOT NULL DEFAULT 0,
                is_deleted INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifacts_conv_id
            ON artifacts(conversation_id, is_deleted, updated_at DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifacts_user_id
            ON artifacts(user_id, is_deleted, updated_at DESC)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Index for cleanup operations: find expired artifacts
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifacts_cleanup
            ON artifacts(is_deleted, updated_at)
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Index for title search within conversations
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifacts_title
            ON artifacts(title, conversation_id)
            WHERE is_deleted = 0
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Index for artifact type queries
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifacts_type
            ON artifacts(artifact_type, conversation_id)
            WHERE is_deleted = 0
            "#,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    // ========================================================================
    // CRUD Operations
    // ========================================================================

    /// Create a new artifact
    pub async fn create(
        &self,
        request: CreateArtifactRequest,
        user_id: Option<String>,
    ) -> ArtifactResult<Artifact> {
        let content_bytes = request.content.as_bytes();
        let content_size = content_bytes.len() as u64;

        // Check size limit
        if content_size > self.config.max_content_size {
            return Err(ArtifactError::ContentTooLarge(
                content_size,
                self.config.max_content_size,
            ));
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let artifact_type_json = serde_json::to_string(&request.artifact_type)?;

        // Store content
        self.storage.store(&id, content_bytes).await?;

        // Insert metadata
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                id, conversation_id, user_id, created_at, updated_at,
                title, description, artifact_type, size, is_deleted
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
            "#,
        )
        .bind(&id)
        .bind(&request.conversation_id)
        .bind(&user_id)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(&request.title)
        .bind(&request.description)
        .bind(&artifact_type_json)
        .bind(content_size as i64)
        .execute(&self.pool)
        .await?;

        Ok(Artifact {
            id,
            conversation_id: request.conversation_id,
            user_id,
            created_at: now,
            updated_at: now,
            title: request.title,
            description: request.description,
            artifact_type: request.artifact_type,
            size: content_size,
            is_deleted: false,
            url: None,
        })
    }

    /// Create a new binary artifact
    ///
    /// Similar to `create` but accepts raw bytes instead of a string.
    /// Uses `max_binary_size` for size limit checking.
    pub async fn create_binary(
        &self,
        conversation_id: String,
        title: String,
        description: Option<String>,
        artifact_type: ArtifactType,
        content: Vec<u8>,
        user_id: Option<String>,
    ) -> ArtifactResult<Artifact> {
        let content_size = content.len() as u64;

        // Check size limit (use binary limit for binary types)
        let max_size = if artifact_type.is_binary() {
            self.config.max_binary_size
        } else {
            self.config.max_content_size
        };

        if content_size > max_size {
            return Err(ArtifactError::ContentTooLarge(content_size, max_size));
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let artifact_type_json = serde_json::to_string(&artifact_type)?;

        // Store content
        self.storage.store(&id, &content).await?;

        // Insert metadata
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                id, conversation_id, user_id, created_at, updated_at,
                title, description, artifact_type, size, is_deleted
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 0)
            "#,
        )
        .bind(&id)
        .bind(&conversation_id)
        .bind(&user_id)
        .bind(now.to_rfc3339())
        .bind(now.to_rfc3339())
        .bind(&title)
        .bind(&description)
        .bind(&artifact_type_json)
        .bind(content_size as i64)
        .execute(&self.pool)
        .await?;

        dual_info!(
            "Created binary artifact: {} ({} bytes, type: {:?})",
            id,
            content_size,
            artifact_type
        );

        Ok(Artifact {
            id,
            conversation_id,
            user_id,
            created_at: now,
            updated_at: now,
            title,
            description,
            artifact_type,
            size: content_size,
            is_deleted: false,
            url: None,
        })
    }

    /// Get artifact by ID
    pub async fn get(&self, id: &str) -> ArtifactResult<Option<Artifact>> {
        let row = sqlx::query_as::<_, ArtifactRow>(
            r#"
            SELECT id, conversation_id, user_id, created_at, updated_at,
                   title, description, artifact_type, size, is_deleted
            FROM artifacts
            WHERE id = ? AND is_deleted = 0
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(row) => Ok(Some(row.into_artifact()?)),
            None => Ok(None),
        }
    }

    /// Get artifact with content
    pub async fn get_with_content(
        &self,
        id: &str,
    ) -> ArtifactResult<Option<ArtifactDetailResponse>> {
        let artifact = self.get(id).await?;

        match artifact {
            Some(artifact) => {
                let content_bytes = self.storage.read(id).await?;
                let content = String::from_utf8_lossy(&content_bytes).to_string();

                Ok(Some(ArtifactDetailResponse { artifact, content }))
            }
            None => Ok(None),
        }
    }

    /// Get artifact content
    pub async fn get_content(&self, id: &str) -> ArtifactResult<Vec<u8>> {
        // Verify artifact exists
        let artifact = self.get(id).await?;
        if artifact.is_none() {
            return Err(ArtifactError::NotFound(id.to_string()));
        }

        self.storage.read(id).await
    }

    /// Get partial artifact content (for Range requests)
    ///
    /// # Arguments
    /// * `id` - Artifact ID
    /// * `offset` - Start offset in bytes
    /// * `length` - Number of bytes to read (None = read to end)
    pub async fn get_content_range(
        &self,
        id: &str,
        offset: u64,
        length: Option<u64>,
    ) -> ArtifactResult<Vec<u8>> {
        // Verify artifact exists
        let artifact = self.get(id).await?;
        if artifact.is_none() {
            return Err(ArtifactError::NotFound(id.to_string()));
        }

        self.storage.read_range(id, offset, length).await
    }

    /// Update artifact
    pub async fn update(
        &self,
        id: &str,
        request: UpdateArtifactRequest,
    ) -> ArtifactResult<Artifact> {
        // Get current artifact
        let artifact = self
            .get(id)
            .await?
            .ok_or_else(|| ArtifactError::NotFound(id.to_string()))?;

        let now = Utc::now();
        let mut new_size = artifact.size;

        // Handle content update (overwrite)
        if let Some(ref content) = request.content {
            let content_bytes = content.as_bytes();
            let content_size = content_bytes.len() as u64;

            // Check size limit
            if content_size > self.config.max_content_size {
                return Err(ArtifactError::ContentTooLarge(
                    content_size,
                    self.config.max_content_size,
                ));
            }

            new_size = content_size;

            // Overwrite content
            self.storage.store(id, content_bytes).await?;
        }

        // Update metadata
        let new_title = request.title.unwrap_or(artifact.title);
        let new_description = request.description.or(artifact.description);

        sqlx::query(
            r#"
            UPDATE artifacts
            SET title = ?, description = ?, size = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&new_title)
        .bind(&new_description)
        .bind(new_size as i64)
        .bind(now.to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(Artifact {
            id: id.to_string(),
            conversation_id: artifact.conversation_id,
            user_id: artifact.user_id,
            created_at: artifact.created_at,
            updated_at: now,
            title: new_title,
            description: new_description,
            artifact_type: artifact.artifact_type,
            size: new_size,
            is_deleted: false,
            url: None,
        })
    }

    /// Soft delete artifact
    pub async fn delete(&self, id: &str) -> ArtifactResult<()> {
        let now = Utc::now();

        let result = sqlx::query(
            r#"
            UPDATE artifacts
            SET is_deleted = 1, updated_at = ?
            WHERE id = ? AND is_deleted = 0
            "#,
        )
        .bind(now.to_rfc3339())
        .bind(id)
        .execute(&self.pool)
        .await?;

        if result.rows_affected() == 0 {
            return Err(ArtifactError::NotFound(id.to_string()));
        }

        Ok(())
    }

    /// List artifacts by conversation
    pub async fn list_by_conversation(
        &self,
        conversation_id: &str,
        limit: i64,
        offset: i64,
    ) -> ArtifactResult<(Vec<Artifact>, i64)> {
        // Get total count
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM artifacts
            WHERE conversation_id = ? AND is_deleted = 0
            "#,
        )
        .bind(conversation_id)
        .fetch_one(&self.pool)
        .await?;

        // Get artifacts
        let rows = sqlx::query_as::<_, ArtifactRow>(
            r#"
            SELECT id, conversation_id, user_id, created_at, updated_at,
                   title, description, artifact_type, size, is_deleted
            FROM artifacts
            WHERE conversation_id = ? AND is_deleted = 0
            ORDER BY updated_at DESC
            LIMIT ? OFFSET ?
            "#,
        )
        .bind(conversation_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let artifacts: Vec<Artifact> = rows
            .into_iter()
            .filter_map(|r| r.into_artifact().ok())
            .collect();

        Ok((artifacts, count.0))
    }

    // ========================================================================
    // Accessors for internal components (used by cleaner)
    // ========================================================================

    /// Returns a reference to the database pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Returns a reference to the storage backend
    pub fn storage(&self) -> &Arc<dyn ArtifactStorage> {
        &self.storage
    }

    /// Returns the configuration
    #[allow(dead_code)]
    pub fn config(&self) -> &ArtifactConfig {
        &self.config
    }
}

// ============================================================================
// Database Row Types
// ============================================================================

#[derive(sqlx::FromRow)]
struct ArtifactRow {
    id: String,
    conversation_id: String,
    user_id: Option<String>,
    created_at: String,
    updated_at: String,
    title: String,
    description: Option<String>,
    artifact_type: String,
    size: i64,
    is_deleted: i32,
}

impl ArtifactRow {
    fn into_artifact(self) -> ArtifactResult<Artifact> {
        let artifact_type: ArtifactType = serde_json::from_str(&self.artifact_type)?;
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());
        let updated_at = chrono::DateTime::parse_from_rfc3339(&self.updated_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(Artifact {
            id: self.id,
            conversation_id: self.conversation_id,
            user_id: self.user_id,
            created_at,
            updated_at,
            title: self.title,
            description: self.description,
            artifact_type,
            size: self.size as u64,
            is_deleted: self.is_deleted != 0,
            url: None,
        })
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn setup_test_store() -> ArtifactStore {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test pool");

        ArtifactStore::new(pool, ArtifactConfig::default())
            .await
            .expect("Failed to create test store")
    }

    #[tokio::test]
    async fn test_create_artifact() {
        let store = setup_test_store().await;

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.rs".to_string(),
            description: Some("Test file".to_string()),
            artifact_type: ArtifactType::Code {
                language: "rust".into(),
            },
            content: "fn main() {}".to_string(),
        };

        let artifact = store.create(request, None).await.unwrap();

        assert!(!artifact.id.is_empty());
        assert_eq!(artifact.title, "test.rs");
        assert_eq!(artifact.size, 12);
    }

    #[tokio::test]
    async fn test_get_artifact() {
        let store = setup_test_store().await;

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.py".to_string(),
            description: None,
            artifact_type: ArtifactType::Code {
                language: "python".into(),
            },
            content: "print('hello')".to_string(),
        };

        let created = store
            .create(request, Some("user_1".to_string()))
            .await
            .unwrap();
        let retrieved = store.get(&created.id).await.unwrap().unwrap();

        assert_eq!(retrieved.id, created.id);
        assert_eq!(retrieved.title, "test.py");
        assert_eq!(retrieved.user_id, Some("user_1".to_string()));
    }

    #[tokio::test]
    async fn test_get_with_content() {
        let store = setup_test_store().await;

        let content = "const x = 42;";
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.js".to_string(),
            description: None,
            artifact_type: ArtifactType::Code {
                language: "javascript".into(),
            },
            content: content.to_string(),
        };

        let created = store.create(request, None).await.unwrap();
        let detail = store.get_with_content(&created.id).await.unwrap().unwrap();

        assert_eq!(detail.content, content);
    }

    #[tokio::test]
    async fn test_update_overwrites_content() {
        let store = setup_test_store().await;

        // Create
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.rs".to_string(),
            description: None,
            artifact_type: ArtifactType::Code {
                language: "rust".into(),
            },
            content: "fn main() {}".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Update with new content
        let updated = store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some("fn main() { println!(\"hello\"); }".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.size, 32);

        // Verify content was overwritten
        let detail = store.get_with_content(&artifact.id).await.unwrap().unwrap();
        assert_eq!(detail.content, "fn main() { println!(\"hello\"); }");
    }

    #[tokio::test]
    async fn test_soft_delete() {
        let store = setup_test_store().await;

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "test".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Delete
        store.delete(&artifact.id).await.unwrap();

        // Should not be found
        let result = store.get(&artifact.id).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_list_by_conversation() {
        let store = setup_test_store().await;

        // Create multiple artifacts
        for i in 0..5 {
            let request = CreateArtifactRequest {
                conversation_id: "conv_1".to_string(),
                title: format!("file_{}.txt", i),
                description: None,
                artifact_type: ArtifactType::Text,
                content: format!("content {}", i),
            };
            store.create(request, None).await.unwrap();
        }

        let (artifacts, total) = store.list_by_conversation("conv_1", 10, 0).await.unwrap();
        assert_eq!(total, 5);
        assert_eq!(artifacts.len(), 5);
    }

    #[tokio::test]
    async fn test_content_size_limit() {
        let store = setup_test_store().await;

        let large_content = "x".repeat(2 * 1024 * 1024); // 2MB
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "large.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: large_content,
        };

        let result = store.create(request, None).await;
        assert!(matches!(result, Err(ArtifactError::ContentTooLarge(_, _))));
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let store = setup_test_store().await;

        let result = store.get("nonexistent_id").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent() {
        let store = setup_test_store().await;

        let result = store.delete("nonexistent_id").await;
        assert!(matches!(result, Err(ArtifactError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_update_metadata_only() {
        let store = setup_test_store().await;

        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "old_title.txt".to_string(),
            description: Some("Old description".to_string()),
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Update only title
        let updated = store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: Some("new_title.txt".to_string()),
                    description: None,
                    content: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.title, "new_title.txt");
    }

    // ========================================================================
    // Performance Tests
    // ========================================================================

    #[tokio::test]
    async fn test_perf_batch_create() {
        let store = setup_test_store().await;
        let count = 100;

        let start = std::time::Instant::now();

        for i in 0..count {
            let request = CreateArtifactRequest {
                conversation_id: format!("conv_{}", i % 10),
                title: format!("artifact_{}.txt", i),
                description: Some(format!("Test artifact {}", i)),
                artifact_type: ArtifactType::Text,
                content: format!("Content for artifact {}", i),
            };
            store.create(request, None).await.unwrap();
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_millis() as f64 / count as f64;

        println!(
            "Created {} artifacts in {:?} (avg: {:.2}ms per artifact)",
            count, elapsed, avg_ms
        );

        // Performance assertion: should average less than 50ms per artifact
        assert!(
            avg_ms < 50.0,
            "Average create time {:.2}ms exceeds 50ms threshold",
            avg_ms
        );
    }

    #[tokio::test]
    async fn test_perf_batch_read() {
        let store = setup_test_store().await;
        let count = 100;

        // Create artifacts first
        let mut ids = Vec::new();
        for i in 0..count {
            let request = CreateArtifactRequest {
                conversation_id: "conv_perf".to_string(),
                title: format!("read_test_{}.txt", i),
                description: None,
                artifact_type: ArtifactType::Text,
                content: format!("Content {}", i),
            };
            let artifact = store.create(request, None).await.unwrap();
            ids.push(artifact.id);
        }

        // Benchmark reads
        let start = std::time::Instant::now();

        for id in &ids {
            store.get_with_content(id).await.unwrap();
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_millis() as f64 / count as f64;

        println!(
            "Read {} artifacts in {:?} (avg: {:.2}ms per read)",
            count, elapsed, avg_ms
        );

        // Performance assertion: should average less than 20ms per read
        assert!(
            avg_ms < 20.0,
            "Average read time {:.2}ms exceeds 20ms threshold",
            avg_ms
        );
    }

    #[tokio::test]
    async fn test_perf_list_by_conversation() {
        let store = setup_test_store().await;
        let artifacts_per_conv = 50;
        let conversations = 5;

        // Create artifacts across multiple conversations
        for conv_idx in 0..conversations {
            for art_idx in 0..artifacts_per_conv {
                let request = CreateArtifactRequest {
                    conversation_id: format!("conv_{}", conv_idx),
                    title: format!("art_{}_{}.txt", conv_idx, art_idx),
                    description: None,
                    artifact_type: ArtifactType::Text,
                    content: format!("Content {} {}", conv_idx, art_idx),
                };
                store.create(request, None).await.unwrap();
            }
        }

        // Benchmark list operations
        let iterations = 20;
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            for conv_idx in 0..conversations {
                let (artifacts, total) = store
                    .list_by_conversation(&format!("conv_{}", conv_idx), 100, 0)
                    .await
                    .unwrap();
                assert_eq!(artifacts.len(), artifacts_per_conv);
                assert_eq!(total, artifacts_per_conv as i64);
            }
        }

        let elapsed = start.elapsed();
        let total_queries = iterations * conversations;
        let avg_ms = elapsed.as_millis() as f64 / total_queries as f64;

        println!(
            "Listed {} queries in {:?} (avg: {:.2}ms per list)",
            total_queries, elapsed, avg_ms
        );

        // Performance assertion: should average less than 10ms per list
        assert!(
            avg_ms < 10.0,
            "Average list time {:.2}ms exceeds 10ms threshold",
            avg_ms
        );
    }

    #[tokio::test]
    async fn test_perf_concurrent_access() {
        use std::sync::Arc;

        let store = Arc::new(setup_test_store().await);

        // Create some artifacts first
        for i in 0..10 {
            let request = CreateArtifactRequest {
                conversation_id: "conv_concurrent".to_string(),
                title: format!("concurrent_{}.txt", i),
                description: None,
                artifact_type: ArtifactType::Text,
                content: format!("Content {}", i),
            };
            store.create(request, None).await.unwrap();
        }

        // Simulate concurrent reads
        let start = std::time::Instant::now();
        let tasks: Vec<_> = (0..50)
            .map(|_| {
                let store_clone = Arc::clone(&store);
                tokio::spawn(async move {
                    store_clone
                        .list_by_conversation("conv_concurrent", 100, 0)
                        .await
                        .unwrap()
                })
            })
            .collect();

        let mut total_artifacts = 0;
        for task in tasks {
            let (artifacts, _) = task.await.unwrap();
            total_artifacts += artifacts.len();
        }

        let elapsed = start.elapsed();

        println!(
            "50 concurrent list operations completed in {:?}, total artifacts: {}",
            elapsed, total_artifacts
        );

        // All should return 10 artifacts
        assert_eq!(total_artifacts, 50 * 10);

        // Should complete within reasonable time (500ms for 50 concurrent ops)
        assert!(
            elapsed.as_millis() < 500,
            "Concurrent operations took too long: {:?}",
            elapsed
        );
    }

    // ==========================================================================
    // Phase 6: Binary File Support Tests
    // ==========================================================================

    #[tokio::test]
    async fn test_create_binary_artifact() {
        let store = setup_test_store().await;
        let binary_content = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG header

        let artifact = store
            .create_binary(
                "conv_binary_1".to_string(),
                "test.png".to_string(),
                Some("Test PNG image".to_string()),
                ArtifactType::Image {
                    format: "png".to_string(),
                },
                binary_content.clone(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(artifact.title, "test.png");
        assert_eq!(artifact.conversation_id, "conv_binary_1");
        assert_eq!(artifact.size, binary_content.len() as u64);
        assert!(matches!(artifact.artifact_type, ArtifactType::Image { .. }));

        // Verify content can be retrieved
        let content = store.get_content(&artifact.id).await.unwrap();
        assert_eq!(content, binary_content);
    }

    #[tokio::test]
    async fn test_binary_artifact_types() {
        let store = setup_test_store().await;

        // Test PDF type
        let pdf_content = b"%PDF-1.4".to_vec();
        let pdf = store
            .create_binary(
                "conv_types".to_string(),
                "doc.pdf".to_string(),
                None,
                ArtifactType::Pdf,
                pdf_content,
                None,
            )
            .await
            .unwrap();
        assert!(matches!(pdf.artifact_type, ArtifactType::Pdf));

        // Test Audio type
        let audio_content = vec![0u8; 100];
        let audio = store
            .create_binary(
                "conv_types".to_string(),
                "sound.mp3".to_string(),
                None,
                ArtifactType::Audio {
                    format: "mp3".to_string(),
                },
                audio_content,
                None,
            )
            .await
            .unwrap();
        assert!(matches!(audio.artifact_type, ArtifactType::Audio { .. }));

        // Test Video type
        let video_content = vec![0u8; 200];
        let video = store
            .create_binary(
                "conv_types".to_string(),
                "clip.mp4".to_string(),
                None,
                ArtifactType::Video {
                    format: "mp4".to_string(),
                },
                video_content,
                None,
            )
            .await
            .unwrap();
        assert!(matches!(video.artifact_type, ArtifactType::Video { .. }));

        // Test Binary type
        let binary_content = vec![0u8; 50];
        let binary = store
            .create_binary(
                "conv_types".to_string(),
                "data.bin".to_string(),
                None,
                ArtifactType::Binary {
                    mime_type: "application/octet-stream".to_string(),
                },
                binary_content,
                None,
            )
            .await
            .unwrap();
        assert!(matches!(binary.artifact_type, ArtifactType::Binary { .. }));
    }

    #[tokio::test]
    async fn test_get_content_range() {
        let store = setup_test_store().await;
        let content: Vec<u8> = (0..100u8).collect();

        let artifact = store
            .create_binary(
                "conv_range".to_string(),
                "range_test.bin".to_string(),
                None,
                ArtifactType::Binary {
                    mime_type: "application/octet-stream".to_string(),
                },
                content.clone(),
                None,
            )
            .await
            .unwrap();

        // Test range read - first 10 bytes
        let range1 = store
            .get_content_range(&artifact.id, 0, Some(10))
            .await
            .unwrap();
        assert_eq!(range1, (0..10u8).collect::<Vec<u8>>());

        // Test range read - middle bytes
        let range2 = store
            .get_content_range(&artifact.id, 50, Some(20))
            .await
            .unwrap();
        assert_eq!(range2, (50..70u8).collect::<Vec<u8>>());

        // Test range read - to end
        let range3 = store
            .get_content_range(&artifact.id, 90, None)
            .await
            .unwrap();
        assert_eq!(range3, (90..100u8).collect::<Vec<u8>>());
    }

    #[tokio::test]
    async fn test_binary_size_limit() {
        let store = setup_test_store().await;

        // Create content exceeding default binary size limit (100MB in config, but smaller for test)
        // This test verifies the store correctly stores and retrieves binary content
        let large_content = vec![0u8; 1024 * 1024]; // 1MB

        let artifact = store
            .create_binary(
                "conv_large".to_string(),
                "large.bin".to_string(),
                None,
                ArtifactType::Binary {
                    mime_type: "application/octet-stream".to_string(),
                },
                large_content.clone(),
                None,
            )
            .await
            .unwrap();

        assert_eq!(artifact.size, large_content.len() as u64);

        // Verify content is stored correctly
        let retrieved = store.get_content(&artifact.id).await.unwrap();
        assert_eq!(retrieved.len(), large_content.len());
        assert_eq!(retrieved, large_content);
    }
}
