use std::io::Write;

use super::pipeline::IndexPipeline;
use crate::{
    chunking::MarkdownChunker,
    embedding::MockEmbedding,
    store::{Database, schema},
};

const DIMS: usize = 64;

/// 创建测试用的 Database + vec 表
fn test_db() -> Database {
    let db = Database::open_in_memory().unwrap();
    schema::ensure_vec_table(db.conn(), DIMS).unwrap();
    db
}

/// 在临时目录中创建 .md 文件
fn write_md(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
    path
}

#[tokio::test]
async fn test_index_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let db = test_db();
    let chunker = MarkdownChunker::new(50, 5);
    let mock = MockEmbedding::new(DIMS);
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    let report = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();

    assert_eq!(report.files_added, 0);
    assert_eq!(report.files_deleted, 0);
    assert_eq!(report.chunks_added, 0);
}

#[tokio::test]
async fn test_index_single_file() {
    let tmp = tempfile::tempdir().unwrap();
    write_md(
        tmp.path(),
        "guide.md",
        "# Guide\n\nThis is a guide.\n\n## Setup\n\nSetup instructions.\n",
    );

    let db = test_db();
    let chunker = MarkdownChunker::new(50, 1);
    let mock = MockEmbedding::new(DIMS);
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    let report = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();

    assert_eq!(report.files_added, 1);
    assert!(report.chunks_added >= 1);

    // 验证数据库中有记录
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_files, 1);
    assert!(stats.total_chunks >= 1);
}

#[tokio::test]
async fn test_incremental_index() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_md(tmp.path(), "doc.md", "# Doc\n\nOriginal content.\n");

    let db = test_db();
    let chunker = MarkdownChunker::new(50, 1);
    let mock = MockEmbedding::new(DIMS);
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    // 首次索引
    let r1 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r1.files_added, 1);
    let chunks_first = r1.chunks_added;

    // 不修改，再次索引 — 应无变化
    let r2 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r2.files_added, 0);
    assert_eq!(r2.files_updated, 0);
    assert_eq!(r2.files_unchanged, 1);

    // 修改文件
    std::fs::write(
        &path,
        "# Doc\n\nUpdated content.\n\n## New Section\n\nNew text.\n",
    )
    .unwrap();

    // 再次索引 — 应检测到变更
    let r3 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r3.files_updated, 1);
    assert!(r3.chunks_added >= 1);
    // 旧 chunks 应被删除
    assert_eq!(r3.chunks_deleted, chunks_first);
}

#[tokio::test]
async fn test_deleted_file_cleanup() {
    let tmp = tempfile::tempdir().unwrap();
    let path = write_md(tmp.path(), "temp.md", "# Temp\n\nTemporary content.\n");

    let db = test_db();
    let chunker = MarkdownChunker::new(50, 1);
    let mock = MockEmbedding::new(DIMS);
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    // 索引
    let r1 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r1.files_added, 1);

    // 删除文件
    std::fs::remove_file(&path).unwrap();

    // 再次索引 — 应清理
    let r2 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r2.files_deleted, 1);
    assert!(r2.chunks_deleted >= 1);

    // 数据库应为空
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_files, 0);
    assert_eq!(stats.total_chunks, 0);
}

#[tokio::test]
async fn test_embedding_cache_hit() {
    let tmp = tempfile::tempdir().unwrap();
    write_md(tmp.path(), "cached.md", "# Cache\n\nTest caching.\n");

    let db = test_db();
    let chunker = MarkdownChunker::new(50, 1);
    let mock = MockEmbedding::new(DIMS);
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    // 首次索引 — 写入 embedding cache
    pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();

    let stats1 = db.get_stats().unwrap();
    assert!(stats1.total_cached_embeddings >= 1);

    // 修改文件内容（但保持相同的 chunk 内容部分不变 — 这里直接测缓存写入正常）
    // 删除文件再重新索引，缓存应仍在
    let cache_count = stats1.total_cached_embeddings;

    // 再次索引相同内容 — 不需要重新计算（文件未变化，直接跳过）
    let r2 = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();
    assert_eq!(r2.files_unchanged, 1);

    // 缓存数量应不变
    let stats2 = db.get_stats().unwrap();
    assert_eq!(stats2.total_cached_embeddings, cache_count);
}

#[tokio::test]
async fn test_large_batch_split() {
    let tmp = tempfile::tempdir().unwrap();

    // 创建一个有很多小段的文件，超过 mock 的 max_batch_size
    let mut content = String::new();
    // MockEmbedding 默认 max_batch_size = 256
    // 用 min_chunk_lines=1 确保每个 heading 独立成 chunk
    for i in 0..10 {
        content.push_str(&format!("## Section {i}\n\nContent for section {i}.\n\n"));
    }
    write_md(tmp.path(), "many_sections.md", &content);

    let db = test_db();
    let chunker = MarkdownChunker::new(50, 1);

    // 使用一个 max_batch_size=3 的 mock 来测试分批
    struct SmallBatchMock;

    #[async_trait::async_trait]
    impl crate::embedding::EmbeddingProvider for SmallBatchMock {
        fn name(&self) -> &str {
            "small-batch"
        }
        fn model(&self) -> &str {
            "small-batch-mock"
        }
        fn dimensions(&self) -> usize {
            DIMS
        }
        fn max_batch_size(&self) -> usize {
            3
        }
        async fn embed_batch(&self, texts: &[&str]) -> crate::error::LantaiResult<Vec<Vec<f32>>> {
            // 验证每批不超过 max_batch_size
            assert!(texts.len() <= 3, "Batch size {} exceeds max 3", texts.len());
            Ok(texts.iter().map(|_| vec![0.1f32; DIMS]).collect())
        }
    }

    let mock = SmallBatchMock;
    let pipeline = IndexPipeline::new(&db, &chunker, &mock);

    let report = pipeline
        .index_directories(&[tmp.path().to_str().unwrap()])
        .await
        .unwrap();

    assert_eq!(report.files_added, 1);
    assert_eq!(report.chunks_added, 10);
}
