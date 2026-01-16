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
    /// Maximum content size for text artifacts (default: 1MB)
    pub max_content_size: u64,
    /// Maximum content size for binary artifacts (default: 100MB)
    pub max_binary_size: u64,
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
            max_content_size: 1024 * 1024,      // 1MB for text
            max_binary_size: 100 * 1024 * 1024, // 100MB for binary
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
        let content_hash = Self::compute_hash(&content);
        let artifact_type_json = serde_json::to_string(&artifact_type)?;

        // Store content
        self.storage.store(&id, 1, &content).await?;

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

    /// Get partial artifact content for a specific version (for Range requests)
    ///
    /// # Arguments
    /// * `id` - Artifact ID
    /// * `version` - Version number
    /// * `offset` - Start offset in bytes
    /// * `length` - Number of bytes to read (None = read to end)
    pub async fn get_content_range(
        &self,
        id: &str,
        version: i32,
        offset: u64,
        length: Option<u64>,
    ) -> ArtifactResult<Vec<u8>> {
        // Verify artifact exists
        let artifact = self.get(id).await?;
        if artifact.is_none() {
            return Err(ArtifactError::NotFound(id.to_string()));
        }

        self.storage.read_range(id, version, offset, length).await
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

    // ========================================================================
    // Performance Tests (P5.4)
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
    async fn test_perf_version_operations() {
        let store = setup_test_store().await;
        let version_count = 20;

        // Create artifact with multiple versions
        let request = CreateArtifactRequest {
            conversation_id: "conv_version_perf".to_string(),
            title: "versioned.txt".to_string(),
            description: None,
            artifact_type: ArtifactType::Text,
            content: "Version 1".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Create multiple versions
        for i in 2..=version_count {
            store
                .update(
                    &artifact.id,
                    UpdateArtifactRequest {
                        title: None,
                        description: None,
                        content: Some(format!("Version {} with more content here", i)),
                        change_description: Some(format!("Update to version {}", i)),
                    },
                )
                .await
                .unwrap();
        }

        // Benchmark get_versions
        let iterations = 50;
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            let versions = store.get_versions(&artifact.id).await.unwrap();
            assert!(!versions.is_empty());
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_millis() as f64 / iterations as f64;

        println!(
            "get_versions {} iterations in {:?} (avg: {:.2}ms)",
            iterations, elapsed, avg_ms
        );

        // Performance assertion: should average less than 5ms
        assert!(
            avg_ms < 5.0,
            "Average get_versions time {:.2}ms exceeds 5ms threshold",
            avg_ms
        );

        // Benchmark get_version_content
        let start = std::time::Instant::now();

        for _ in 0..iterations {
            // Read random version
            let version = (iterations % version_count) as i32 + 1;
            store
                .get_version_content(&artifact.id, version)
                .await
                .unwrap();
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_millis() as f64 / iterations as f64;

        println!(
            "get_version_content {} iterations in {:?} (avg: {:.2}ms)",
            iterations, elapsed, avg_ms
        );

        // Performance assertion: should average less than 10ms
        assert!(
            avg_ms < 10.0,
            "Average get_version_content time {:.2}ms exceeds 10ms threshold",
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
        let content = store
            .get_content(&artifact.id, artifact.version)
            .await
            .unwrap();
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
            .get_content_range(&artifact.id, artifact.version, 0, Some(10))
            .await
            .unwrap();
        assert_eq!(range1, (0..10u8).collect::<Vec<u8>>());

        // Test range read - middle bytes
        let range2 = store
            .get_content_range(&artifact.id, artifact.version, 50, Some(20))
            .await
            .unwrap();
        assert_eq!(range2, (50..70u8).collect::<Vec<u8>>());

        // Test range read - to end
        let range3 = store
            .get_content_range(&artifact.id, artifact.version, 90, None)
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
        let retrieved = store
            .get_content(&artifact.id, artifact.version)
            .await
            .unwrap();
        assert_eq!(retrieved.len(), large_content.len());
        assert_eq!(retrieved, large_content);
    }

    #[tokio::test]
    async fn test_binary_versioning() {
        let store = setup_test_store().await;

        // Create initial version
        let content_v1 = vec![1u8; 50];
        let artifact = store
            .create_binary(
                "conv_versioned".to_string(),
                "versioned.bin".to_string(),
                None,
                ArtifactType::Binary {
                    mime_type: "application/octet-stream".to_string(),
                },
                content_v1.clone(),
                None,
            )
            .await
            .unwrap();

        // Update with new binary content
        let content_v2 = vec![2u8; 75];
        let updated = store
            .update(
                &artifact.id,
                UpdateArtifactRequest {
                    title: None,
                    description: None,
                    content: Some(String::from_utf8_lossy(&content_v2).to_string()),
                    change_description: None,
                },
            )
            .await
            .unwrap();

        assert_eq!(updated.version, 2);

        // Get specific version content
        let v1_response = store.get_version_content(&artifact.id, 1).await.unwrap();
        assert_eq!(v1_response.artifact.version, 1);

        let v2_response = store.get_version_content(&artifact.id, 2).await.unwrap();
        assert_eq!(v2_response.artifact.version, 2);
    }
}
