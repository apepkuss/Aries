//! Artifact Lifecycle Cleaner
//!
//! Background service for cleaning up expired and soft-deleted artifacts.
//! Handles:
//! - Expired artifacts (based on retention_days)
//! - Soft-deleted artifacts past retention period
//! - Orphaned content (content without metadata)

use std::sync::Arc;

use chrono::{Duration, Utc};
use tokio::time::{Duration as TokioDuration, interval};

use super::store::{ArtifactConfig, ArtifactResult, ArtifactStore};
use crate::{dual_error, dual_info};

/// Artifact cleanup service
///
/// Runs periodic cleanup tasks to manage artifact lifecycle.
pub struct ArtifactCleaner {
    store: Arc<ArtifactStore>,
    config: ArtifactConfig,
}

impl ArtifactCleaner {
    /// Creates a new ArtifactCleaner
    pub fn new(store: Arc<ArtifactStore>, config: ArtifactConfig) -> Self {
        Self { store, config }
    }

    /// Starts the background cleanup task
    ///
    /// Returns a JoinHandle that can be used to abort the task if needed.
    pub fn start(self) -> tokio::task::JoinHandle<()> {
        let interval_secs = self.config.cleanup_interval_secs;

        tokio::spawn(async move {
            dual_info!(
                "Artifact cleaner started (interval: {}s, retention: {}d, soft-delete: {}d)",
                interval_secs,
                self.config.retention_days,
                self.config.soft_delete_retention_days
            );

            let mut timer = interval(TokioDuration::from_secs(interval_secs));

            loop {
                timer.tick().await;

                if let Err(e) = self.run_cleanup().await {
                    dual_error!("Artifact cleanup failed: {}", e);
                }
            }
        })
    }

    /// Runs a single cleanup cycle
    ///
    /// This can also be called manually for testing or immediate cleanup.
    pub async fn run_cleanup(&self) -> ArtifactResult<CleanupStats> {
        dual_info!("Starting artifact cleanup cycle...");

        let mut stats = CleanupStats::default();

        // 1. Mark expired artifacts as deleted
        if self.config.retention_days > 0 {
            stats.expired = self.cleanup_expired().await?;
        }

        // 2. Physically delete soft-deleted artifacts past retention
        stats.purged = self.cleanup_soft_deleted().await?;

        // 3. Clean up orphaned content
        stats.orphaned = self.cleanup_orphaned_content().await?;

        dual_info!(
            "Artifact cleanup completed: {} expired, {} purged, {} orphaned",
            stats.expired,
            stats.purged,
            stats.orphaned
        );

        Ok(stats)
    }

    /// Marks expired artifacts as soft-deleted
    async fn cleanup_expired(&self) -> ArtifactResult<usize> {
        let retention_days = self.config.retention_days as i64;
        let cutoff = Utc::now() - Duration::days(retention_days);
        let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();

        let result = sqlx::query(
            r#"
            UPDATE artifacts
            SET is_deleted = TRUE, updated_at = CURRENT_TIMESTAMP
            WHERE updated_at < ? AND is_deleted = FALSE
            "#,
        )
        .bind(&cutoff_str)
        .execute(self.store.pool())
        .await?;

        let count = result.rows_affected() as usize;
        if count > 0 {
            dual_info!(
                "Marked {} artifacts as expired (cutoff: {})",
                count,
                cutoff_str
            );
        }

        Ok(count)
    }

    /// Physically deletes soft-deleted artifacts past retention period
    async fn cleanup_soft_deleted(&self) -> ArtifactResult<usize> {
        let retention_days = self.config.soft_delete_retention_days as i64;
        let cutoff = Utc::now() - Duration::days(retention_days);
        let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();

        // Get IDs of artifacts to purge
        let ids: Vec<String> = sqlx::query_scalar(
            r#"
            SELECT id FROM artifacts
            WHERE is_deleted = TRUE AND updated_at < ?
            "#,
        )
        .bind(&cutoff_str)
        .fetch_all(self.store.pool())
        .await?;

        if ids.is_empty() {
            return Ok(0);
        }

        let count = ids.len();
        dual_info!(
            "Purging {} soft-deleted artifacts (cutoff: {})",
            count,
            cutoff_str
        );

        // Delete each artifact's content and metadata
        for id in &ids {
            // Delete from storage backend
            if let Err(e) = self.store.storage().delete_all(id).await {
                dual_error!("Failed to delete storage for artifact {}: {}", id, e);
                // Continue with other artifacts
            }

            // Delete version records
            sqlx::query("DELETE FROM artifact_versions WHERE artifact_id = ?")
                .bind(id)
                .execute(self.store.pool())
                .await?;

            // Delete metadata
            sqlx::query("DELETE FROM artifacts WHERE id = ?")
                .bind(id)
                .execute(self.store.pool())
                .await?;
        }

        Ok(count)
    }

    /// Cleans up orphaned content (content without metadata)
    async fn cleanup_orphaned_content(&self) -> ArtifactResult<usize> {
        // Delete version records without parent artifact
        let result = sqlx::query(
            r#"
            DELETE FROM artifact_versions
            WHERE artifact_id NOT IN (SELECT id FROM artifacts)
            "#,
        )
        .execute(self.store.pool())
        .await?;

        let count = result.rows_affected() as usize;
        if count > 0 {
            dual_info!("Cleaned up {} orphaned version records", count);
        }

        Ok(count)
    }
}

/// Statistics from a cleanup cycle
#[derive(Debug, Default, Clone)]
pub struct CleanupStats {
    /// Number of artifacts marked as expired
    pub expired: usize,
    /// Number of soft-deleted artifacts physically purged
    pub purged: usize,
    /// Number of orphaned content records cleaned
    pub orphaned: usize,
}

impl CleanupStats {
    /// Returns total items cleaned
    #[allow(dead_code)]
    pub fn total(&self) -> usize {
        self.expired + self.purged + self.orphaned
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use super::*;

    async fn setup_test_store() -> Arc<ArtifactStore> {
        let pool = SqlitePoolOptions::new()
            .connect("sqlite::memory:")
            .await
            .expect("Failed to create test pool");

        let config = ArtifactConfig {
            retention_days: 1,
            soft_delete_retention_days: 1,
            cleanup_interval_secs: 3600,
            enable_cleanup: true,
            ..Default::default()
        };

        Arc::new(
            ArtifactStore::new(pool, config)
                .await
                .expect("Failed to create store"),
        )
    }

    #[tokio::test]
    async fn test_cleaner_creation() {
        let store = setup_test_store().await;
        let config = ArtifactConfig::default();

        let cleaner = ArtifactCleaner::new(store, config);
        assert_eq!(cleaner.config.retention_days, 30);
    }

    #[tokio::test]
    async fn test_cleanup_empty_database() {
        let store = setup_test_store().await;
        let config = ArtifactConfig {
            retention_days: 1,
            soft_delete_retention_days: 1,
            ..Default::default()
        };

        let cleaner = ArtifactCleaner::new(store, config);
        let stats = cleaner.run_cleanup().await.unwrap();

        assert_eq!(stats.total(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_expired_artifacts() {
        let store = setup_test_store().await;

        // Create an artifact
        let request = crate::artifacts::CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: crate::artifacts::ArtifactType::Text,
            content: "test content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Manually set updated_at to past (simulate old artifact)
        let old_date = (Utc::now() - Duration::days(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query("UPDATE artifacts SET updated_at = ? WHERE id = ?")
            .bind(&old_date)
            .bind(&artifact.id)
            .execute(store.pool())
            .await
            .unwrap();

        // Run cleanup with 1-day retention
        let config = ArtifactConfig {
            retention_days: 1,
            soft_delete_retention_days: 1,
            ..Default::default()
        };

        let cleaner = ArtifactCleaner::new(store.clone(), config);
        let stats = cleaner.run_cleanup().await.unwrap();

        assert_eq!(stats.expired, 1);

        // Verify artifact is marked as deleted
        let result = store.get(&artifact.id).await.unwrap();
        assert!(result.is_none()); // get() filters out deleted
    }

    #[tokio::test]
    async fn test_cleanup_soft_deleted_artifacts() {
        let store = setup_test_store().await;

        // Create and soft-delete an artifact
        let request = crate::artifacts::CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: crate::artifacts::ArtifactType::Text,
            content: "test content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();
        store.delete(&artifact.id).await.unwrap();

        // Set updated_at to past (simulate old soft-delete)
        let old_date = (Utc::now() - Duration::days(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query("UPDATE artifacts SET updated_at = ? WHERE id = ?")
            .bind(&old_date)
            .bind(&artifact.id)
            .execute(store.pool())
            .await
            .unwrap();

        // Run cleanup
        let config = ArtifactConfig {
            retention_days: 0, // No expiration
            soft_delete_retention_days: 1,
            ..Default::default()
        };

        let cleaner = ArtifactCleaner::new(store.clone(), config);
        let stats = cleaner.run_cleanup().await.unwrap();

        assert_eq!(stats.purged, 1);

        // Verify artifact is completely removed
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM artifacts WHERE id = ?")
            .bind(&artifact.id)
            .fetch_one(store.pool())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_cleanup_orphaned_with_cascade() {
        // Note: Due to ON DELETE CASCADE foreign key constraint,
        // deleting an artifact automatically deletes its version records.
        // This test verifies cleanup_orphaned_content handles the case
        // when there are no orphaned records (which is the expected state).
        let store = setup_test_store().await;

        // Create an artifact
        let request = crate::artifacts::CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "cascade_test.txt".to_string(),
            description: None,
            artifact_type: crate::artifacts::ArtifactType::Text,
            content: "content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Delete the artifact (soft delete)
        store.delete(&artifact.id).await.unwrap();

        // Set updated_at to past so it qualifies for purging
        let old_date = (Utc::now() - Duration::days(10))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query("UPDATE artifacts SET updated_at = ? WHERE id = ?")
            .bind(&old_date)
            .bind(&artifact.id)
            .execute(store.pool())
            .await
            .unwrap();

        // Run cleanup with 1-day soft delete retention
        let config = ArtifactConfig {
            retention_days: 0, // No auto-expiry
            soft_delete_retention_days: 1,
            ..Default::default()
        };

        let cleaner = ArtifactCleaner::new(store.clone(), config);
        let stats = cleaner.run_cleanup().await.unwrap();

        // With CASCADE, there should be no orphans to clean
        // But there should be 1 purged (the soft-deleted artifact)
        assert_eq!(stats.purged, 1);
        assert_eq!(stats.orphaned, 0);
    }

    #[tokio::test]
    async fn test_cleanup_stats_total() {
        let stats = CleanupStats {
            expired: 5,
            purged: 3,
            orphaned: 2,
        };
        assert_eq!(stats.total(), 10);
    }

    #[tokio::test]
    async fn test_no_cleanup_when_retention_zero() {
        let store = setup_test_store().await;

        // Create an artifact
        let request = crate::artifacts::CreateArtifactRequest {
            conversation_id: "conv_1".to_string(),
            title: "test.txt".to_string(),
            description: None,
            artifact_type: crate::artifacts::ArtifactType::Text,
            content: "test content".to_string(),
        };
        let artifact = store.create(request, None).await.unwrap();

        // Set updated_at to past
        let old_date = (Utc::now() - Duration::days(100))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        sqlx::query("UPDATE artifacts SET updated_at = ? WHERE id = ?")
            .bind(&old_date)
            .bind(&artifact.id)
            .execute(store.pool())
            .await
            .unwrap();

        // Run cleanup with retention_days = 0 (never expire)
        let config = ArtifactConfig {
            retention_days: 0,
            soft_delete_retention_days: 30,
            ..Default::default()
        };

        let cleaner = ArtifactCleaner::new(store.clone(), config);
        let stats = cleaner.run_cleanup().await.unwrap();

        // Should not expire any artifacts
        assert_eq!(stats.expired, 0);

        // Artifact should still exist
        let result = store.get(&artifact.id).await.unwrap();
        assert!(result.is_some());
    }
}
