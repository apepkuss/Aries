//! Storage Backend Abstraction
//!
//! Provides a trait-based abstraction for artifact content storage,
//! supporting both local filesystem and S3-compatible backends.

mod filesystem;

use async_trait::async_trait;
pub use filesystem::FileSystemStorage;

use super::ArtifactResult;

// ============================================================================
// Storage Stats
// ============================================================================

/// Storage statistics
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StorageStats {
    /// Total storage size in bytes
    pub total_size: u64,
    /// Number of files
    pub file_count: u64,
    /// Available space (filesystem only)
    pub available_space: Option<u64>,
}

// ============================================================================
// Storage Trait
// ============================================================================

/// Storage backend abstraction trait
///
/// All storage implementations must implement this trait to ensure
/// seamless switching between backends.
#[async_trait]
pub trait ArtifactStorage: Send + Sync {
    /// Store content
    ///
    /// # Arguments
    /// - `artifact_id`: Artifact unique identifier
    /// - `version`: Version number
    /// - `content`: Content bytes
    async fn store(&self, artifact_id: &str, version: i32, content: &[u8]) -> ArtifactResult<()>;

    /// Read content
    async fn read(&self, artifact_id: &str, version: i32) -> ArtifactResult<Vec<u8>>;

    /// Delete a specific version
    async fn delete(&self, artifact_id: &str, version: i32) -> ArtifactResult<()>;

    /// Delete all versions of an artifact
    #[allow(dead_code)]
    async fn delete_all(&self, artifact_id: &str) -> ArtifactResult<()>;

    /// Check if a version exists
    #[allow(dead_code)]
    async fn exists(&self, artifact_id: &str, version: i32) -> ArtifactResult<bool>;

    /// Get storage statistics
    #[allow(dead_code)]
    async fn stats(&self) -> ArtifactResult<StorageStats>;

    /// Get backend name (for logging and debugging)
    fn backend_name(&self) -> &'static str;
}
