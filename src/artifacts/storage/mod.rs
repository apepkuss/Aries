//! Storage Backend Abstraction
//!
//! Provides a trait-based abstraction for artifact content storage,
//! supporting both local filesystem and S3-compatible backends.

mod filesystem;

use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
pub use filesystem::FileSystemStorage;
use futures_util::Stream;

use super::ArtifactResult;

/// Type alias for a boxed byte stream
pub type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>;

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
    /// - `content`: Content bytes
    async fn store(&self, artifact_id: &str, content: &[u8]) -> ArtifactResult<()>;

    /// Read content
    async fn read(&self, artifact_id: &str) -> ArtifactResult<Vec<u8>>;

    /// Delete artifact content
    async fn delete(&self, artifact_id: &str) -> ArtifactResult<()>;

    /// Delete all files of an artifact
    #[allow(dead_code)]
    async fn delete_all(&self, artifact_id: &str) -> ArtifactResult<()>;

    /// Check if artifact content exists
    #[allow(dead_code)]
    async fn exists(&self, artifact_id: &str) -> ArtifactResult<bool>;

    /// Get storage statistics
    #[allow(dead_code)]
    async fn stats(&self) -> ArtifactResult<StorageStats>;

    /// Get backend name (for logging and debugging)
    fn backend_name(&self) -> &'static str;

    // ========================================================================
    // Streaming Methods (Phase 6)
    // ========================================================================

    /// Read a range of bytes from content (for HTTP Range requests)
    ///
    /// # Arguments
    /// * `artifact_id` - Artifact ID
    /// * `offset` - Start offset in bytes
    /// * `length` - Number of bytes to read (None = read to end)
    #[allow(dead_code)]
    async fn read_range(
        &self,
        artifact_id: &str,
        offset: u64,
        length: Option<u64>,
    ) -> ArtifactResult<Vec<u8>>;

    /// Get file size without reading content
    #[allow(dead_code)]
    async fn file_size(&self, artifact_id: &str) -> ArtifactResult<u64>;

    /// Store content from a stream (for large file uploads)
    ///
    /// # Arguments
    /// * `artifact_id` - Artifact ID
    /// * `stream` - Byte stream to read from
    #[allow(dead_code)]
    async fn store_stream(&self, artifact_id: &str, stream: ByteStream) -> ArtifactResult<u64>;

    /// Read content as a stream (for large file downloads)
    ///
    /// # Arguments
    /// * `artifact_id` - Artifact ID
    /// * `chunk_size` - Size of each chunk in bytes
    #[allow(dead_code)]
    async fn read_stream(&self, artifact_id: &str, chunk_size: usize)
    -> ArtifactResult<ByteStream>;
}
