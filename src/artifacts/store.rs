//! Artifact Store - Database and Storage Coordination
//!
//! Manages artifact metadata (SQLite) and content storage (pluggable backend).

use std::sync::Arc;

use chrono::Utc;
use sha2::{Digest, Sha256};
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

    #[error("Version not found: artifact={0}, version={1}")]
    VersionNotFound(String, i32),

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
    /// Maximum content size (default: 1MB)
    pub max_content_size: u64,
    /// Maximum versions to keep per artifact (default: 10)
    pub max_versions: i32,
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
            max_content_size: 1024 * 1024, // 1MB
            max_versions: 10,
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
                version INTEGER NOT NULL DEFAULT 1,
                size INTEGER NOT NULL DEFAULT 0,
                is_deleted INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Artifact versions table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS artifact_versions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                artifact_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                content_hash TEXT NOT NULL,
                size INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                change_description TEXT,
                FOREIGN KEY (artifact_id) REFERENCES artifacts(id) ON DELETE CASCADE,
                UNIQUE(artifact_id, version)
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

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_artifact_versions_artifact
            ON artifact_versions(artifact_id, version DESC)
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
        let content_hash = Self::compute_hash(content_bytes);
        let artifact_type_json = serde_json::to_string(&request.artifact_type)?;

        // Store content
        self.storage.store(&id, 1, content_bytes).await?;

        // Insert metadata
        sqlx::query(
            r#"
            INSERT INTO artifacts (
                id, conversation_id, user_id, created_at, updated_at,
                title, description, artifact_type, version, size, is_deleted
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, 1, ?, 0)
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

        // Insert version record
        sqlx::query(
            r#"
            INSERT INTO artifact_versions (
                artifact_id, version, content_hash, size, created_at, change_description
            ) VALUES (?, 1, ?, ?, ?, 'Initial version')
            "#,
        )
        .bind(&id)
        .bind(&content_hash)
        .bind(content_size as i64)
        .bind(now.to_rfc3339())
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
            version: 1,
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
                   title, description, artifact_type, version, size, is_deleted
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
                let content_bytes = self.storage.read(id, artifact.version).await?;
                let content = String::from_utf8_lossy(&content_bytes).to_string();

                Ok(Some(ArtifactDetailResponse { artifact, content }))
            }
            None => Ok(None),
        }
    }

    /// Get artifact content for a specific version
    pub async fn get_content(&self, id: &str, version: i32) -> ArtifactResult<Vec<u8>> {
        // Verify artifact exists
        let artifact = self.get(id).await?;
        if artifact.is_none() {
            return Err(ArtifactError::NotFound(id.to_string()));
        }

        self.storage.read(id, version).await
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
        let mut new_version = artifact.version;
        let mut new_size = artifact.size;

        // Handle content update (creates new version)
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

            new_version = artifact.version + 1;
            new_size = content_size;
            let content_hash = Self::compute_hash(content_bytes);

            // Store new version content
            self.storage.store(id, new_version, content_bytes).await?;

            // Insert version record
            sqlx::query(
                r#"
                INSERT INTO artifact_versions (
                    artifact_id, version, content_hash, size, created_at, change_description
                ) VALUES (?, ?, ?, ?, ?, ?)
                "#,
            )
            .bind(id)
            .bind(new_version)
            .bind(&content_hash)
            .bind(content_size as i64)
            .bind(now.to_rfc3339())
            .bind(&request.change_description)
            .execute(&self.pool)
            .await?;

            // Clean up old versions if needed
            self.cleanup_old_versions(id).await?;
        }

        // Update metadata
        let new_title = request.title.unwrap_or(artifact.title);
        let new_description = request.description.or(artifact.description);

        sqlx::query(
            r#"
            UPDATE artifacts
            SET title = ?, description = ?, version = ?, size = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(&new_title)
        .bind(&new_description)
        .bind(new_version)
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
            version: new_version,
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
                   title, description, artifact_type, version, size, is_deleted
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

    /// Get version history for an artifact
    pub async fn get_versions(&self, artifact_id: &str) -> ArtifactResult<Vec<ArtifactVersion>> {
        let rows = sqlx::query_as::<_, ArtifactVersionRow>(
            r#"
            SELECT id, artifact_id, version, content_hash, size, created_at, change_description
            FROM artifact_versions
            WHERE artifact_id = ?
            ORDER BY version DESC
            "#,
        )
        .bind(artifact_id)
        .fetch_all(&self.pool)
        .await?;

        let versions: Vec<ArtifactVersion> = rows
            .into_iter()
            .filter_map(|r| r.into_version().ok())
            .collect();

        Ok(versions)
    }

    /// Restore artifact to a specific version
    ///
    /// Creates a new version with the content from the specified version.
    /// This is a non-destructive operation - the old versions are preserved.
    pub async fn restore_version(
        &self,
        artifact_id: &str,
        target_version: i32,
    ) -> ArtifactResult<Artifact> {
        // Get current artifact
        let artifact = self
            .get(artifact_id)
            .await?
            .ok_or_else(|| ArtifactError::NotFound(artifact_id.to_string()))?;

        // Verify target version exists
        let version_exists: Option<(i32,)> = sqlx::query_as(
            r#"
            SELECT version FROM artifact_versions
            WHERE artifact_id = ? AND version = ?
            "#,
        )
        .bind(artifact_id)
        .bind(target_version)
        .fetch_optional(&self.pool)
        .await?;

        if version_exists.is_none() {
            return Err(ArtifactError::VersionNotFound(
                artifact_id.to_string(),
                target_version,
            ));
        }

        // If already at target version, return current artifact
        if artifact.version == target_version {
            return Ok(artifact);
        }

        // Read content from target version
        let content_bytes = self.storage.read(artifact_id, target_version).await?;
        let content_size = content_bytes.len() as u64;
        let content_hash = Self::compute_hash(&content_bytes);
        let new_version = artifact.version + 1;
        let now = Utc::now();

        // Store as new version
        self.storage
            .store(artifact_id, new_version, &content_bytes)
            .await?;

        // Insert version record
        let change_desc = format!("Restored from version {}", target_version);
        sqlx::query(
            r#"
            INSERT INTO artifact_versions (
                artifact_id, version, content_hash, size, created_at, change_description
            ) VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(artifact_id)
        .bind(new_version)
        .bind(&content_hash)
        .bind(content_size as i64)
        .bind(now.to_rfc3339())
        .bind(&change_desc)
        .execute(&self.pool)
        .await?;

        // Update artifact metadata
        sqlx::query(
            r#"
            UPDATE artifacts
            SET version = ?, size = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(new_version)
        .bind(content_size as i64)
        .bind(now.to_rfc3339())
        .bind(artifact_id)
        .execute(&self.pool)
        .await?;

        // Clean up old versions if needed
        self.cleanup_old_versions(artifact_id).await?;

        Ok(Artifact {
            id: artifact_id.to_string(),
            conversation_id: artifact.conversation_id,
            user_id: artifact.user_id,
            created_at: artifact.created_at,
            updated_at: now,
            title: artifact.title,
            description: artifact.description,
            artifact_type: artifact.artifact_type,
            version: new_version,
            size: content_size,
            is_deleted: false,
            url: None,
        })
    }

    /// Get content for a specific version
    pub async fn get_version_content(
        &self,
        artifact_id: &str,
        version: i32,
    ) -> ArtifactResult<ArtifactDetailResponse> {
        // Get artifact metadata
        let artifact = self
            .get(artifact_id)
            .await?
            .ok_or_else(|| ArtifactError::NotFound(artifact_id.to_string()))?;

        // Verify version exists
        let version_info: Option<ArtifactVersionRow> = sqlx::query_as(
            r#"
            SELECT id, artifact_id, version, content_hash, size, created_at, change_description
            FROM artifact_versions
            WHERE artifact_id = ? AND version = ?
            "#,
        )
        .bind(artifact_id)
        .bind(version)
        .fetch_optional(&self.pool)
        .await?;

        if version_info.is_none() {
            return Err(ArtifactError::VersionNotFound(
                artifact_id.to_string(),
                version,
            ));
        }

        // Read content from storage
        let content_bytes = self.storage.read(artifact_id, version).await?;
        let content = String::from_utf8_lossy(&content_bytes).to_string();

        // Return artifact with the requested version info
        let mut versioned_artifact = artifact;
        versioned_artifact.version = version;

        Ok(ArtifactDetailResponse {
            artifact: versioned_artifact,
            content,
        })
    }

    // ========================================================================
    // Helper Methods
    // ========================================================================

    /// Compute SHA-256 hash of content
    fn compute_hash(content: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("{:x}", hasher.finalize())
    }

    /// Clean up old versions beyond max_versions limit
    async fn cleanup_old_versions(&self, artifact_id: &str) -> ArtifactResult<()> {
        // Get versions to delete
        let old_versions: Vec<(i32,)> = sqlx::query_as(
            r#"
            SELECT version FROM artifact_versions
            WHERE artifact_id = ?
            ORDER BY version DESC
            LIMIT -1 OFFSET ?
            "#,
        )
        .bind(artifact_id)
        .bind(self.config.max_versions)
        .fetch_all(&self.pool)
        .await?;

        for (version,) in old_versions {
            // Delete from storage
            let _ = self.storage.delete(artifact_id, version).await;

            // Delete from database
            sqlx::query(
                r#"
                DELETE FROM artifact_versions
                WHERE artifact_id = ? AND version = ?
                "#,
            )
            .bind(artifact_id)
            .bind(version)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
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
    version: i32,
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
            version: self.version,
            size: self.size as u64,
            is_deleted: self.is_deleted != 0,
            url: None,
        })
    }
}

#[derive(sqlx::FromRow)]
struct ArtifactVersionRow {
    id: i64,
    artifact_id: String,
    version: i32,
    content_hash: String,
    size: i64,
    created_at: String,
    change_description: Option<String>,
}

impl ArtifactVersionRow {
    fn into_version(self) -> ArtifactResult<ArtifactVersion> {
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(ArtifactVersion {
            id: self.id,
            artifact_id: self.artifact_id,
            version: self.version,
            content_hash: self.content_hash,
            size: self.size as u64,
            created_at,
            change_description: self.change_description,
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
        assert_eq!(artifact.version, 1);
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
    async fn test_update_creates_new_version() {
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
                    change_description: Some("Added print".to_string()),
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.version, 2);

        // Check versions
        let versions = store.get_versions(&artifact.id).await.unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2);
        assert_eq!(versions[1].version, 1);
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
                    change_description: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.title, "new_title.txt");
        assert_eq!(updated.version, 1); // Version should not change
    }

    // ========================================================================
    // Version Management Tests (P2.5)
    // ========================================================================

    #[tokio::test]
    async fn test_restore_version() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "versioned.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "version 1 content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Update to version 2
        store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some("version 2 content".to_string()),
                    change_description: Some("Updated to v2".to_string()),
                },
            )
            .await
            .unwrap();

        // Update to version 3
        store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some("version 3 content".to_string()),
                    change_description: Some("Updated to v3".to_string()),
                },
            )
            .await
            .unwrap();

        // Restore to version 1
        let restored = store.restore_version(&artifact.id, 1).await.unwrap();

        // Should be version 4 (new version with v1 content)
        assert_eq!(restored.version, 4);

        // Content should match version 1
        let detail = store.get_with_content(&artifact.id).await.unwrap().unwrap();
        assert_eq!(detail.content, "version 1 content");

        // Should have 4 versions now
        let versions = store.get_versions(&artifact.id).await.unwrap();
        assert_eq!(versions.len(), 4);

        // Latest version should have restore description
        assert!(
            versions[0]
                .change_description
                .as_ref()
                .unwrap()
                .contains("Restored from version 1")
        );
    }

    #[tokio::test]
    async fn test_restore_version_not_found() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Try to restore to non-existent version
        let result = store.restore_version(&artifact.id, 999).await;
        assert!(matches!(
            result,
            Err(ArtifactError::VersionNotFound(_, 999))
        ));
    }

    #[tokio::test]
    async fn test_restore_current_version_no_op() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Restore to current version (should be no-op)
        let restored = store.restore_version(&artifact.id, 1).await.unwrap();

        assert_eq!(restored.version, 1);

        // Should still have only 1 version
        let versions = store.get_versions(&artifact.id).await.unwrap();
        assert_eq!(versions.len(), 1);
    }

    #[tokio::test]
    async fn test_get_version_content() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "multi_version.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "original content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Update to version 2
        store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some("updated content".to_string()),
                    change_description: None,
                },
            )
            .await
            .unwrap();

        // Get version 1 content
        let v1_detail = store.get_version_content(&artifact.id, 1).await.unwrap();
        assert_eq!(v1_detail.content, "original content");
        assert_eq!(v1_detail.artifact.version, 1);

        // Get version 2 content
        let v2_detail = store.get_version_content(&artifact.id, 2).await.unwrap();
        assert_eq!(v2_detail.content, "updated content");
        assert_eq!(v2_detail.artifact.version, 2);
    }

    #[tokio::test]
    async fn test_get_version_content_not_found() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Try to get non-existent version
        let result = store.get_version_content(&artifact.id, 999).await;
        assert!(matches!(
            result,
            Err(ArtifactError::VersionNotFound(_, 999))
        ));
    }

    #[tokio::test]
    async fn test_version_cleanup() {
        // Create store with max 3 versions
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test pool");

        let config = ArtifactConfig {
            max_content_size: 1024 * 1024,
            max_versions: 3,
            storage_path: None,
            ..Default::default()
        };
        let store = ArtifactStore::new(pool, config).await.unwrap();

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "cleanup_test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "v1".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Create 4 more versions (total 5)
        for i in 2..=5 {
            store
                .update(
                    &artifact.id,
                    UpdateArtifactRequest {
                        title: None,
                        description: None,
                        content: Some(format!("v{}", i)),
                        change_description: None,
                    },
                )
                .await
                .unwrap();
        }

        // Should only have 3 versions (max_versions)
        let versions = store.get_versions(&artifact.id).await.unwrap();
        assert_eq!(versions.len(), 3);

        // Should have versions 5, 4, 3 (newest first)
        assert_eq!(versions[0].version, 5);
        assert_eq!(versions[1].version, 4);
        assert_eq!(versions[2].version, 3);
    }

    #[tokio::test]
    async fn test_version_change_description() {
        let store = setup_test_store().await;

        // Create artifact
        let request = CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "initial".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Update with change description
        store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some("updated".to_string()),
                    change_description: Some("Fixed bug in line 42".to_string()),
                },
            )
            .await
            .unwrap();

        // Check version has correct description
        let versions = store.get_versions(&artifact.id).await.unwrap();
        assert_eq!(
            versions[0].change_description.as_deref(),
            Some("Fixed bug in line 42")
        );
        assert_eq!(
            versions[1].change_description.as_deref(),
            Some("Initial version")
        );
    }
}
