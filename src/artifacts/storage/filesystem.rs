//! File System Storage Backend
//!
//! Default storage implementation using the local filesystem.
//! Zero external dependencies, suitable for local deployment.
//!
//! ## Features
//! - Directory sharding to avoid filesystem bottlenecks
//! - Buffered I/O for large files
//! - Streaming read support for efficient memory usage
//! - Atomic writes with temporary files

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use futures_util::StreamExt;
use tokio::{
    fs,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader, BufWriter},
};
use tokio_util::io::ReaderStream;

use super::{ArtifactStorage, ByteStream, StorageStats};
use crate::artifacts::{ArtifactError, ArtifactResult};

/// Default buffer size for I/O operations (64KB)
const DEFAULT_BUFFER_SIZE: usize = 64 * 1024;

/// File system storage backend
///
/// Suitable for local deployment with zero external dependencies.
pub struct FileSystemStorage {
    /// Storage root directory
    base_path: PathBuf,
}

impl FileSystemStorage {
    /// Create a new instance
    ///
    /// # Arguments
    /// - `base_path`: Storage root directory. If None, uses system default data directory.
    pub async fn new(base_path: Option<PathBuf>) -> ArtifactResult<Self> {
        let base_path = base_path.unwrap_or_else(Self::default_path);

        // Ensure directory exists
        fs::create_dir_all(&base_path).await?;

        Ok(Self { base_path })
    }

    /// Get system default storage path
    fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("moss")
            .join("artifacts")
    }

    /// Build file path: base_path/{shard}/{artifact_id}/content
    ///
    /// Uses first 2 characters of artifact_id for directory sharding
    /// to avoid too many files in a single directory.
    fn file_path(&self, artifact_id: &str) -> PathBuf {
        let shard = if artifact_id.len() >= 2 {
            &artifact_id[0..2]
        } else {
            "00"
        };

        self.base_path.join(shard).join(artifact_id).join("content")
    }

    /// Get artifact directory
    fn artifact_dir(&self, artifact_id: &str) -> PathBuf {
        let shard = if artifact_id.len() >= 2 {
            &artifact_id[0..2]
        } else {
            "00"
        };

        self.base_path.join(shard).join(artifact_id)
    }

    /// Get base path (for testing)
    #[cfg(test)]
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }

    /// Read a range of bytes from a file (for streaming/Range requests)
    #[allow(dead_code)]
    pub async fn read_range(
        &self,
        artifact_id: &str,
        offset: u64,
        length: Option<u64>,
    ) -> ArtifactResult<Vec<u8>> {
        let path = self.file_path(artifact_id);

        if !path.exists() {
            return Err(ArtifactError::NotFound(artifact_id.to_string()));
        }

        let file = fs::File::open(&path).await?;
        let file_size = file.metadata().await?.len();

        // Validate offset
        if offset >= file_size {
            return Ok(Vec::new());
        }

        // Calculate actual length
        let max_length = file_size - offset;
        let read_length = length.map(|l| l.min(max_length)).unwrap_or(max_length) as usize;

        let mut reader = BufReader::with_capacity(DEFAULT_BUFFER_SIZE, file);
        reader.seek(std::io::SeekFrom::Start(offset)).await?;

        let mut buffer = vec![0u8; read_length];
        reader.read_exact(&mut buffer).await?;

        Ok(buffer)
    }

    /// Get file size without reading content
    #[allow(dead_code)]
    pub async fn file_size(&self, artifact_id: &str) -> ArtifactResult<u64> {
        let path = self.file_path(artifact_id);

        if !path.exists() {
            return Err(ArtifactError::NotFound(artifact_id.to_string()));
        }

        let metadata = fs::metadata(&path).await?;
        Ok(metadata.len())
    }

    /// Store content with atomic write (write to temp file, then rename)
    ///
    /// This prevents partial writes if the process is interrupted.
    #[allow(dead_code)]
    pub async fn store_atomic(&self, artifact_id: &str, content: &[u8]) -> ArtifactResult<()> {
        let path = self.file_path(artifact_id);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write to temporary file
        let temp_path = path.with_extension("tmp");
        {
            let file = fs::File::create(&temp_path).await?;
            let mut writer = BufWriter::with_capacity(DEFAULT_BUFFER_SIZE, file);
            writer.write_all(content).await?;
            writer.flush().await?;
        }

        // Atomic rename
        fs::rename(&temp_path, &path).await?;

        Ok(())
    }
}

#[async_trait]
impl ArtifactStorage for FileSystemStorage {
    async fn store(&self, artifact_id: &str, content: &[u8]) -> ArtifactResult<()> {
        let path = self.file_path(artifact_id);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Use buffered writer for better performance with large files
        let file = fs::File::create(&path).await?;
        let mut writer = BufWriter::with_capacity(DEFAULT_BUFFER_SIZE, file);
        writer.write_all(content).await?;
        writer.flush().await?;

        Ok(())
    }

    async fn read(&self, artifact_id: &str) -> ArtifactResult<Vec<u8>> {
        let path = self.file_path(artifact_id);

        if !path.exists() {
            return Err(ArtifactError::NotFound(artifact_id.to_string()));
        }

        // Use buffered reader for better performance
        let file = fs::File::open(&path).await?;
        let mut reader = BufReader::with_capacity(DEFAULT_BUFFER_SIZE, file);
        let mut content = Vec::new();
        reader.read_to_end(&mut content).await?;

        Ok(content)
    }

    async fn delete(&self, artifact_id: &str) -> ArtifactResult<()> {
        let path = self.file_path(artifact_id);

        if path.exists() {
            fs::remove_file(&path).await?;
        }

        // Try to clean up empty directory
        let artifact_dir = self.artifact_dir(artifact_id);
        if artifact_dir.exists() {
            // Ignore error (directory may not be empty)
            let _ = fs::remove_dir(&artifact_dir).await;
        }

        Ok(())
    }

    async fn delete_all(&self, artifact_id: &str) -> ArtifactResult<()> {
        let artifact_dir = self.artifact_dir(artifact_id);

        if artifact_dir.exists() {
            fs::remove_dir_all(&artifact_dir).await?;
        }

        Ok(())
    }

    async fn exists(&self, artifact_id: &str) -> ArtifactResult<bool> {
        let path = self.file_path(artifact_id);
        Ok(path.exists())
    }

    async fn stats(&self) -> ArtifactResult<StorageStats> {
        let mut total_size = 0u64;
        let mut file_count = 0u64;

        // Recursive directory size calculation
        fn count_dir(path: &Path) -> std::io::Result<(u64, u64)> {
            let mut size = 0u64;
            let mut count = 0u64;

            if path.is_dir() {
                for entry in std::fs::read_dir(path)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.is_dir() {
                        let (s, c) = count_dir(&path)?;
                        size += s;
                        count += c;
                    } else {
                        size += entry.metadata()?.len();
                        count += 1;
                    }
                }
            }

            Ok((size, count))
        }

        if self.base_path.exists() {
            let (size, count) = count_dir(&self.base_path).unwrap_or((0, 0));
            total_size = size;
            file_count = count;
        }

        Ok(StorageStats {
            total_size,
            file_count,
            available_space: None,
        })
    }

    fn backend_name(&self) -> &'static str {
        "filesystem"
    }

    async fn read_range(
        &self,
        artifact_id: &str,
        offset: u64,
        length: Option<u64>,
    ) -> ArtifactResult<Vec<u8>> {
        // Delegate to the inherent method
        FileSystemStorage::read_range(self, artifact_id, offset, length).await
    }

    async fn file_size(&self, artifact_id: &str) -> ArtifactResult<u64> {
        // Delegate to the inherent method
        FileSystemStorage::file_size(self, artifact_id).await
    }

    async fn store_stream(&self, artifact_id: &str, mut stream: ByteStream) -> ArtifactResult<u64> {
        let path = self.file_path(artifact_id);

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write to temporary file first for atomicity
        let temp_path = path.with_extension("tmp");
        let file = fs::File::create(&temp_path).await?;
        let mut writer = BufWriter::with_capacity(DEFAULT_BUFFER_SIZE, file);
        let mut total_bytes = 0u64;

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result?;
            writer.write_all(&chunk).await?;
            total_bytes += chunk.len() as u64;
        }

        writer.flush().await?;
        drop(writer);

        // Atomic rename
        fs::rename(&temp_path, &path).await?;

        Ok(total_bytes)
    }

    async fn read_stream(
        &self,
        artifact_id: &str,
        chunk_size: usize,
    ) -> ArtifactResult<ByteStream> {
        let path = self.file_path(artifact_id);

        if !path.exists() {
            return Err(ArtifactError::NotFound(artifact_id.to_string()));
        }

        let file = fs::File::open(&path).await?;
        let reader = BufReader::with_capacity(chunk_size, file);
        let stream = ReaderStream::with_capacity(reader, chunk_size);

        Ok(Box::pin(stream))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use futures_util::StreamExt;
    use tempfile::TempDir;

    use super::*;

    async fn create_test_storage() -> (FileSystemStorage, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let storage = FileSystemStorage::new(Some(temp_dir.path().to_path_buf()))
            .await
            .expect("Failed to create storage");
        (storage, temp_dir)
    }

    #[tokio::test]
    async fn test_store_and_read() {
        let (storage, _temp) = create_test_storage().await;

        let content = b"fn main() { println!(\"Hello\"); }";
        storage.store("art_123", content).await.unwrap();

        let read_content = storage.read("art_123").await.unwrap();
        assert_eq!(read_content, content);
    }

    #[tokio::test]
    async fn test_read_nonexistent() {
        let (storage, _temp) = create_test_storage().await;

        let result = storage.read("nonexistent").await;
        assert!(matches!(result, Err(ArtifactError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_exists() {
        let (storage, _temp) = create_test_storage().await;

        assert!(!storage.exists("art_456").await.unwrap());

        storage.store("art_456", b"content").await.unwrap();

        assert!(storage.exists("art_456").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let (storage, _temp) = create_test_storage().await;

        storage.store("art_789", b"data").await.unwrap();
        assert!(storage.exists("art_789").await.unwrap());

        storage.delete("art_789").await.unwrap();
        assert!(!storage.exists("art_789").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_all() {
        let (storage, _temp) = create_test_storage().await;

        storage.store("art_abc", b"data").await.unwrap();

        storage.delete_all("art_abc").await.unwrap();

        assert!(!storage.exists("art_abc").await.unwrap());
    }

    #[tokio::test]
    async fn test_stats() {
        let (storage, _temp) = create_test_storage().await;

        storage.store("art_1", b"content1").await.unwrap();
        storage.store("art_2", b"content22").await.unwrap();

        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.file_count, 2);
        assert_eq!(stats.total_size, 17); // 8 + 9 bytes
    }

    #[tokio::test]
    async fn test_backend_name() {
        let (storage, _temp) = create_test_storage().await;
        assert_eq!(storage.backend_name(), "filesystem");
    }

    #[tokio::test]
    async fn test_sharding() {
        let (storage, _temp) = create_test_storage().await;

        // Store artifacts with different prefixes
        storage.store("ab_artifact", b"1").await.unwrap();
        storage.store("cd_artifact", b"2").await.unwrap();

        // Verify sharding structure
        let ab_path = storage.base_path().join("ab").join("ab_artifact");
        let cd_path = storage.base_path().join("cd").join("cd_artifact");

        assert!(ab_path.exists());
        assert!(cd_path.exists());
    }

    #[tokio::test]
    async fn test_overwrite_content() {
        let (storage, _temp) = create_test_storage().await;

        storage.store("overwrite", b"version 1").await.unwrap();
        assert_eq!(storage.read("overwrite").await.unwrap(), b"version 1");

        storage.store("overwrite", b"version 2").await.unwrap();
        assert_eq!(storage.read("overwrite").await.unwrap(), b"version 2");
    }

    #[tokio::test]
    async fn test_read_range() {
        let (storage, _temp) = create_test_storage().await;

        let content = b"0123456789ABCDEFGHIJ";
        storage.store("range_test", content).await.unwrap();

        // Read from beginning
        let result = storage.read_range("range_test", 0, Some(5)).await.unwrap();
        assert_eq!(result, b"01234");

        // Read from middle
        let result = storage.read_range("range_test", 10, Some(5)).await.unwrap();
        assert_eq!(result, b"ABCDE");

        // Read to end
        let result = storage.read_range("range_test", 15, None).await.unwrap();
        assert_eq!(result, b"FGHIJ");

        // Read with length exceeding file size
        let result = storage
            .read_range("range_test", 15, Some(100))
            .await
            .unwrap();
        assert_eq!(result, b"FGHIJ");

        // Read past end
        let result = storage
            .read_range("range_test", 100, Some(10))
            .await
            .unwrap();
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_size() {
        let (storage, _temp) = create_test_storage().await;

        let content = b"Hello, World!";
        storage.store("size_test", content).await.unwrap();

        let size = storage.file_size("size_test").await.unwrap();
        assert_eq!(size, 13);
    }

    #[tokio::test]
    async fn test_file_size_nonexistent() {
        let (storage, _temp) = create_test_storage().await;

        let result = storage.file_size("nonexistent").await;
        assert!(matches!(result, Err(ArtifactError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_store_atomic() {
        let (storage, _temp) = create_test_storage().await;

        let content = b"atomic write test content";
        storage.store_atomic("atomic_test", content).await.unwrap();

        let read_content = storage.read("atomic_test").await.unwrap();
        assert_eq!(read_content, content);

        // Temp file should not exist
        let path = storage.file_path("atomic_test");
        let temp_path = path.with_extension("tmp");
        assert!(!temp_path.exists());
    }

    #[tokio::test]
    async fn test_large_file_buffered_io() {
        let (storage, _temp) = create_test_storage().await;

        // Create a 1MB file to test buffered I/O
        let content: Vec<u8> = (0..1024 * 1024).map(|i| (i % 256) as u8).collect();
        storage.store("large_file", &content).await.unwrap();

        let read_content = storage.read("large_file").await.unwrap();
        assert_eq!(read_content.len(), content.len());
        assert_eq!(read_content, content);

        // Test range read on large file
        let middle = storage
            .read_range("large_file", 512 * 1024, Some(1024))
            .await
            .unwrap();
        assert_eq!(middle.len(), 1024);
    }

    // ========================================================================
    // Streaming Tests (Phase 6)
    // ========================================================================

    #[tokio::test]
    async fn test_store_stream() {
        use bytes::Bytes;

        let (storage, _temp) = create_test_storage().await;

        // Create a stream of chunks
        let chunks = vec![
            Ok(Bytes::from("Hello, ")),
            Ok(Bytes::from("World!")),
            Ok(Bytes::from(" This is streaming.")),
        ];
        let stream: super::ByteStream = Box::pin(futures_util::stream::iter(chunks.into_iter()));

        // Store from stream
        let bytes_written = storage.store_stream("stream_test", stream).await.unwrap();
        // "Hello, " (7) + "World!" (6) + " This is streaming." (19) = 32 bytes
        assert_eq!(bytes_written, 32);

        // Verify content
        let content = storage.read("stream_test").await.unwrap();
        assert_eq!(content, b"Hello, World! This is streaming.");
    }

    #[tokio::test]
    async fn test_read_stream() {
        let (storage, _temp) = create_test_storage().await;

        // Store some content
        let original = b"This is test content for streaming read.";
        storage.store("read_stream_test", original).await.unwrap();

        // Read as stream
        let mut stream = storage.read_stream("read_stream_test", 10).await.unwrap();

        // Collect all chunks
        let mut collected = Vec::new();
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected.extend_from_slice(&chunk);
        }

        assert_eq!(collected, original);
    }

    #[tokio::test]
    async fn test_read_stream_large_file() {
        let (storage, _temp) = create_test_storage().await;

        // Create a 100KB file
        let content: Vec<u8> = (0..100 * 1024).map(|i| (i % 256) as u8).collect();
        storage.store("large_stream", &content).await.unwrap();

        // Read with 16KB chunks
        let mut stream = storage
            .read_stream("large_stream", 16 * 1024)
            .await
            .unwrap();

        let mut collected = Vec::new();
        let mut chunk_count = 0;
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.unwrap();
            collected.extend_from_slice(&chunk);
            chunk_count += 1;
        }

        assert_eq!(collected.len(), content.len());
        assert_eq!(collected, content);
        // Should have multiple chunks (100KB / 16KB = ~7 chunks)
        assert!(chunk_count > 1);
    }

    #[tokio::test]
    async fn test_read_stream_nonexistent() {
        let (storage, _temp) = create_test_storage().await;

        let result = storage.read_stream("nonexistent", 1024).await;
        assert!(matches!(result, Err(ArtifactError::NotFound(_))));
    }

    #[tokio::test]
    async fn test_store_stream_empty() {
        let (storage, _temp) = create_test_storage().await;

        // Empty stream
        let stream: super::ByteStream = Box::pin(futures_util::stream::empty());

        let bytes_written = storage.store_stream("empty_stream", stream).await.unwrap();
        assert_eq!(bytes_written, 0);

        // Verify empty file exists
        assert!(storage.exists("empty_stream").await.unwrap());
        let content = storage.read("empty_stream").await.unwrap();
        assert!(content.is_empty());
    }

    #[tokio::test]
    async fn test_trait_read_range() {
        // Test read_range through the trait interface
        let (storage, _temp) = create_test_storage().await;
        let storage: &dyn ArtifactStorage = &storage;

        storage.store("trait_range", b"0123456789").await.unwrap();

        let result = storage.read_range("trait_range", 3, Some(4)).await.unwrap();
        assert_eq!(result, b"3456");
    }

    #[tokio::test]
    async fn test_trait_file_size() {
        // Test file_size through the trait interface
        let (storage, _temp) = create_test_storage().await;
        let storage: &dyn ArtifactStorage = &storage;

        storage.store("trait_size", b"Hello, World!").await.unwrap();

        let size = storage.file_size("trait_size").await.unwrap();
        assert_eq!(size, 13);
    }
}
