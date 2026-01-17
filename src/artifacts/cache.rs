//! Artifact Cache
//!
//! Simple LRU cache implementation for artifact metadata and content.
//! Reduces database queries for frequently accessed artifacts.

use std::{collections::HashMap, sync::Arc, time::Instant};

use tokio::sync::RwLock;

use super::types::Artifact;

/// Cache entry containing artifact metadata and optional content
#[allow(dead_code)]
struct CacheEntry {
    /// Artifact metadata
    artifact: Artifact,
    /// Cached content (for text artifacts)
    content: Option<Vec<u8>>,
    /// Last access time for LRU eviction
    accessed_at: Instant,
    /// Entry size in bytes (for memory tracking)
    size: usize,
}

/// Simple LRU cache for artifacts
///
/// Provides caching for artifact metadata and content to reduce
/// database and storage backend queries.
#[allow(dead_code)]
pub struct ArtifactCache {
    /// Cache storage
    cache: Arc<RwLock<HashMap<String, CacheEntry>>>,
    /// Maximum number of entries
    max_entries: usize,
    /// Maximum total cache size in bytes
    max_size: usize,
    /// Current total size
    current_size: Arc<RwLock<usize>>,
}

#[allow(dead_code)]
impl Default for ArtifactCache {
    /// Creates a cache with default settings (100 entries, 10MB)
    fn default() -> Self {
        Self::new(100, 10 * 1024 * 1024)
    }
}

#[allow(dead_code)]
impl ArtifactCache {
    /// Creates a new ArtifactCache
    ///
    /// # Arguments
    /// * `max_entries` - Maximum number of cached entries
    /// * `max_size` - Maximum total cache size in bytes
    pub fn new(max_entries: usize, max_size: usize) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            max_entries,
            max_size,
            current_size: Arc::new(RwLock::new(0)),
        }
    }

    /// Gets artifact metadata from cache
    pub async fn get(&self, id: &str) -> Option<Artifact> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(id) {
            entry.accessed_at = Instant::now();
            return Some(entry.artifact.clone());
        }
        None
    }

    /// Gets artifact with content from cache
    pub async fn get_with_content(&self, id: &str) -> Option<(Artifact, Option<Vec<u8>>)> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.get_mut(id) {
            entry.accessed_at = Instant::now();
            return Some((entry.artifact.clone(), entry.content.clone()));
        }
        None
    }

    /// Sets artifact in cache
    ///
    /// # Arguments
    /// * `artifact` - Artifact metadata
    /// * `content` - Optional content bytes
    pub async fn set(&self, artifact: Artifact, content: Option<Vec<u8>>) {
        let entry_size = Self::calculate_entry_size(&artifact, &content);

        // Skip if single entry exceeds max size
        if entry_size > self.max_size {
            return;
        }

        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        // Remove existing entry if present
        if let Some(old_entry) = cache.remove(&artifact.id) {
            *current_size = current_size.saturating_sub(old_entry.size);
        }

        // Evict entries until we have space
        while (cache.len() >= self.max_entries || *current_size + entry_size > self.max_size)
            && !cache.is_empty()
        {
            if let Some((evicted_id, evicted_entry)) = Self::find_oldest_entry(&cache) {
                cache.remove(&evicted_id);
                *current_size = current_size.saturating_sub(evicted_entry.size);
            } else {
                break;
            }
        }

        // Insert new entry
        let id = artifact.id.clone();
        cache.insert(
            id,
            CacheEntry {
                artifact,
                content,
                accessed_at: Instant::now(),
                size: entry_size,
            },
        );
        *current_size += entry_size;
    }

    /// Invalidates a cache entry
    pub async fn invalidate(&self, id: &str) {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.remove(id) {
            let mut current_size = self.current_size.write().await;
            *current_size = current_size.saturating_sub(entry.size);
        }
    }

    /// Invalidates all entries for a conversation
    pub async fn invalidate_conversation(&self, conversation_id: &str) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;

        let ids_to_remove: Vec<String> = cache
            .iter()
            .filter(|(_, entry)| entry.artifact.conversation_id == conversation_id)
            .map(|(id, _)| id.clone())
            .collect();

        for id in ids_to_remove {
            if let Some(entry) = cache.remove(&id) {
                *current_size = current_size.saturating_sub(entry.size);
            }
        }
    }

    /// Clears the entire cache
    pub async fn clear(&self) {
        let mut cache = self.cache.write().await;
        let mut current_size = self.current_size.write().await;
        cache.clear();
        *current_size = 0;
    }

    /// Returns cache statistics
    pub async fn stats(&self) -> CacheStats {
        let cache = self.cache.read().await;
        let current_size = self.current_size.read().await;

        CacheStats {
            entries: cache.len(),
            max_entries: self.max_entries,
            current_size: *current_size,
            max_size: self.max_size,
        }
    }

    /// Calculates entry size in bytes
    fn calculate_entry_size(artifact: &Artifact, content: &Option<Vec<u8>>) -> usize {
        // Base size for artifact metadata (approximate)
        let mut size = 256; // Fixed overhead
        size += artifact.id.len();
        size += artifact.conversation_id.len();
        size += artifact.title.len();
        if let Some(ref desc) = artifact.description {
            size += desc.len();
        }
        if let Some(ref user_id) = artifact.user_id {
            size += user_id.len();
        }

        // Content size
        if let Some(content) = content {
            size += content.len();
        }

        size
    }

    /// Finds the oldest entry for eviction
    fn find_oldest_entry(cache: &HashMap<String, CacheEntry>) -> Option<(String, CacheEntry)> {
        cache
            .iter()
            .min_by_key(|(_, entry)| entry.accessed_at)
            .map(|(id, entry)| {
                (
                    id.clone(),
                    CacheEntry {
                        artifact: entry.artifact.clone(),
                        content: entry.content.clone(),
                        accessed_at: entry.accessed_at,
                        size: entry.size,
                    },
                )
            })
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CacheStats {
    /// Current number of entries
    pub entries: usize,
    /// Maximum entries allowed
    pub max_entries: usize,
    /// Current cache size in bytes
    pub current_size: usize,
    /// Maximum cache size in bytes
    pub max_size: usize,
}

#[allow(dead_code)]
impl CacheStats {
    /// Returns cache utilization as a percentage
    pub fn utilization(&self) -> f64 {
        if self.max_size == 0 {
            0.0
        } else {
            (self.current_size as f64 / self.max_size as f64) * 100.0
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::artifacts::ArtifactType;

    fn create_test_artifact(id: &str, conv_id: &str) -> Artifact {
        Artifact {
            id: id.to_string(),
            conversation_id: conv_id.to_string(),
            user_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            title: format!("Test {}", id),
            description: None,
            artifact_type: ArtifactType::Text,
            version: 1,
            size: 100,
            is_deleted: false,
            url: None,
        }
    }

    #[tokio::test]
    async fn test_cache_creation() {
        let cache = ArtifactCache::new(10, 1024);
        let stats = cache.stats().await;
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.max_entries, 10);
    }

    #[tokio::test]
    async fn test_cache_set_and_get() {
        let cache = ArtifactCache::new(10, 1024 * 1024);
        let artifact = create_test_artifact("art_1", "conv_1");

        // Set
        cache.set(artifact.clone(), None).await;

        // Get
        let result = cache.get("art_1").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "art_1");
    }

    #[tokio::test]
    async fn test_cache_get_with_content() {
        let cache = ArtifactCache::new(10, 1024 * 1024);
        let artifact = create_test_artifact("art_1", "conv_1");
        let content = b"test content".to_vec();

        cache.set(artifact, Some(content.clone())).await;

        let result = cache.get_with_content("art_1").await;
        assert!(result.is_some());
        let (art, cont) = result.unwrap();
        assert_eq!(art.id, "art_1");
        assert_eq!(cont, Some(content));
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let cache = ArtifactCache::new(10, 1024 * 1024);
        let result = cache.get("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate() {
        let cache = ArtifactCache::new(10, 1024 * 1024);
        let artifact = create_test_artifact("art_1", "conv_1");

        cache.set(artifact, None).await;
        assert!(cache.get("art_1").await.is_some());

        cache.invalidate("art_1").await;
        assert!(cache.get("art_1").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_invalidate_conversation() {
        let cache = ArtifactCache::new(10, 1024 * 1024);

        // Add artifacts from two conversations
        cache
            .set(create_test_artifact("art_1", "conv_1"), None)
            .await;
        cache
            .set(create_test_artifact("art_2", "conv_1"), None)
            .await;
        cache
            .set(create_test_artifact("art_3", "conv_2"), None)
            .await;

        // Invalidate conv_1
        cache.invalidate_conversation("conv_1").await;

        // conv_1 artifacts should be gone
        assert!(cache.get("art_1").await.is_none());
        assert!(cache.get("art_2").await.is_none());

        // conv_2 artifact should remain
        assert!(cache.get("art_3").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_clear() {
        let cache = ArtifactCache::new(10, 1024 * 1024);

        cache
            .set(create_test_artifact("art_1", "conv_1"), None)
            .await;
        cache
            .set(create_test_artifact("art_2", "conv_1"), None)
            .await;

        cache.clear().await;

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.current_size, 0);
    }

    #[tokio::test]
    async fn test_cache_lru_eviction() {
        let cache = ArtifactCache::new(2, 1024 * 1024); // Only 2 entries

        // Add 3 artifacts
        cache
            .set(create_test_artifact("art_1", "conv_1"), None)
            .await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        cache
            .set(create_test_artifact("art_2", "conv_1"), None)
            .await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        cache
            .set(create_test_artifact("art_3", "conv_1"), None)
            .await;

        // art_1 should be evicted (oldest)
        assert!(cache.get("art_1").await.is_none());
        assert!(cache.get("art_2").await.is_some());
        assert!(cache.get("art_3").await.is_some());
    }

    #[tokio::test]
    async fn test_cache_size_limit_eviction() {
        // Small cache size limit
        let cache = ArtifactCache::new(100, 1000);

        let artifact = create_test_artifact("art_1", "conv_1");
        let large_content = vec![0u8; 500];

        cache.set(artifact.clone(), Some(large_content)).await;

        let artifact2 = create_test_artifact("art_2", "conv_1");
        let large_content2 = vec![0u8; 500];

        cache.set(artifact2, Some(large_content2)).await;

        // First entry might be evicted due to size constraints
        let stats = cache.stats().await;
        assert!(stats.current_size <= cache.max_size);
    }

    #[tokio::test]
    async fn test_cache_stats() {
        let cache = ArtifactCache::new(10, 1024 * 1024);

        cache
            .set(create_test_artifact("art_1", "conv_1"), None)
            .await;
        cache
            .set(
                create_test_artifact("art_2", "conv_1"),
                Some(b"content".to_vec()),
            )
            .await;

        let stats = cache.stats().await;
        assert_eq!(stats.entries, 2);
        assert!(stats.current_size > 0);
        assert!(stats.utilization() > 0.0);
    }

    #[tokio::test]
    async fn test_cache_update_existing() {
        let cache = ArtifactCache::new(10, 1024 * 1024);

        let mut artifact = create_test_artifact("art_1", "conv_1");
        cache.set(artifact.clone(), Some(b"v1".to_vec())).await;

        // Update with new content
        artifact.version = 2;
        cache.set(artifact, Some(b"v2".to_vec())).await;

        let result = cache.get_with_content("art_1").await;
        assert!(result.is_some());
        let (art, content) = result.unwrap();
        assert_eq!(art.version, 2);
        assert_eq!(content, Some(b"v2".to_vec()));

        // Should still be only 1 entry
        let stats = cache.stats().await;
        assert_eq!(stats.entries, 1);
    }

    #[tokio::test]
    async fn test_cache_stats_utilization() {
        let stats = CacheStats {
            entries: 5,
            max_entries: 10,
            current_size: 500,
            max_size: 1000,
        };
        assert!((stats.utilization() - 50.0).abs() < 0.01);
    }
}
