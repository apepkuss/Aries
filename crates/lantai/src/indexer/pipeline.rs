use std::path::Path;

use super::sync::compute_sync_plan;
use crate::{
    chunking::{
        MarkdownChunker,
        types::{ScannedFile, content_hash, generate_composite_id},
    },
    embedding::EmbeddingProvider,
    error::{LantaiError, LantaiResult},
    store::{Database, chunks::ChunkRecord, files::FileRecord, schema},
};

/// 索引报告
#[derive(Debug, Clone, Default)]
pub struct IndexReport {
    pub files_added: usize,
    pub files_updated: usize,
    pub files_deleted: usize,
    pub files_unchanged: usize,
    pub chunks_added: usize,
    pub chunks_deleted: usize,
}

/// 索引管线：扫描 → 分块 → 嵌入 → 存储
pub struct IndexPipeline<'a> {
    db: &'a Database,
    chunker: &'a MarkdownChunker,
    embedding: &'a dyn EmbeddingProvider,
}

impl<'a> IndexPipeline<'a> {
    pub fn new(
        db: &'a Database,
        chunker: &'a MarkdownChunker,
        embedding: &'a dyn EmbeddingProvider,
    ) -> Self {
        Self {
            db,
            chunker,
            embedding,
        }
    }

    /// 索引指定目录
    pub async fn index_directories(&self, dirs: &[&str]) -> LantaiResult<IndexReport> {
        // 确保 vec 表存在
        schema::ensure_vec_table(&self.db.conn(), self.embedding.dimensions())?;

        // 1. 扫描目录，收集所有 .md 文件
        let scanned = self.scan_directories(dirs)?;

        // 2. 计算同步计划
        let plan = compute_sync_plan(self.db, &scanned)?;

        let mut report = IndexReport {
            files_unchanged: plan.unchanged_count,
            ..Default::default()
        };

        // 3. 删除已移除文件的 chunks
        for path in &plan.deleted_files {
            let old_chunks = self.db.get_chunk_ids_by_file(path)?;
            report.chunks_deleted += old_chunks.len();
            self.db.delete_chunks_by_file(path)?;
            self.db.delete_file(path)?;
            report.files_deleted += 1;
        }

        // 4. 处理变更文件
        for file in &plan.changed_files {
            let is_update = self.db.get_file(&file.path)?.is_some();
            let file_report = self.index_single_file(file).await?;
            report.chunks_added += file_report.chunks_added;
            report.chunks_deleted += file_report.chunks_deleted;
            if is_update {
                report.files_updated += 1;
            } else {
                report.files_added += 1;
            }
        }

        Ok(report)
    }

    /// 索引单个文件
    async fn index_single_file(&self, file: &ScannedFile) -> LantaiResult<IndexReport> {
        let mut report = IndexReport::default();

        // 1. 读取文件内容
        let content = std::fs::read_to_string(&file.path)?;

        // 2. 语义分块
        let chunks = self.chunker.chunk_file(&file.path, &content);

        // 3. 删除该文件旧的 chunks
        let old_chunks = self.db.get_chunk_ids_by_file(&file.path)?;
        report.chunks_deleted = old_chunks.len();
        if !old_chunks.is_empty() {
            self.db.delete_chunks_by_file(&file.path)?;
        }

        // 4. 先写入 files 表（chunks 有 FK 约束引用 files.path）
        let now = chrono::Utc::now().to_rfc3339();
        self.db.upsert_file(&FileRecord {
            path: file.path.clone(),
            content_hash: file.content_hash.clone(),
            modified_at: file.modified_at.to_rfc3339(),
            indexed_at: now,
        })?;

        // 5. 计算 embedding（优先查缓存）
        let embeddings = self.compute_embeddings(&chunks).await?;

        // 6. 构建 ChunkRecord 并写入数据库
        let model = self.embedding.model();
        let chunk_records: Vec<ChunkRecord> = chunks
            .iter()
            .zip(embeddings.into_iter())
            .map(|(chunk, emb)| {
                let composite_id = generate_composite_id(
                    &chunk.source_path,
                    chunk.start_line,
                    chunk.end_line,
                    &chunk.content_hash,
                    model,
                );
                ChunkRecord {
                    composite_id,
                    source_path: chunk.source_path.clone(),
                    heading_path: chunk.heading_path.clone(),
                    content: chunk.content.clone(),
                    start_line: chunk.start_line as i64,
                    end_line: chunk.end_line as i64,
                    content_hash: chunk.content_hash.clone(),
                    embedding_model: model.to_string(),
                    embedding: Some(emb),
                }
            })
            .collect();

        report.chunks_added = chunk_records.len();
        self.db.insert_chunks(&chunk_records)?;

        Ok(report)
    }

    /// 批量计算 embedding，利用缓存避免重复计算
    async fn compute_embeddings(
        &self,
        chunks: &[crate::chunking::Chunk],
    ) -> LantaiResult<Vec<Vec<f32>>> {
        let model = self.embedding.model();
        let mut results: Vec<Vec<f32>> = Vec::with_capacity(chunks.len());
        let mut uncached_indices = Vec::new();
        let mut uncached_texts = Vec::new();

        // 1. 查缓存
        for (i, chunk) in chunks.iter().enumerate() {
            if let Some(cached) = self.db.get_cached_embedding(&chunk.content_hash, model)? {
                results.push(cached);
            } else {
                uncached_indices.push(i);
                uncached_texts.push(chunk.content.as_str());
                results.push(Vec::new()); // 占位
            }
        }

        // 2. 批量计算未缓存的
        if !uncached_texts.is_empty() {
            let batch_size = self.embedding.max_batch_size();
            for batch_start in (0..uncached_texts.len()).step_by(batch_size) {
                let batch_end = (batch_start + batch_size).min(uncached_texts.len());
                let batch = &uncached_texts[batch_start..batch_end];
                let embeddings = self.embedding.embed_batch(batch).await?;

                for (j, emb) in embeddings.into_iter().enumerate() {
                    let idx = uncached_indices[batch_start + j];
                    // 3. 写入缓存
                    self.db
                        .cache_embedding(&chunks[idx].content_hash, model, &emb)?;
                    results[idx] = emb;
                }
            }
        }

        Ok(results)
    }

    /// 递归扫描目录，收集所有 .md 文件
    fn scan_directories(&self, dirs: &[&str]) -> LantaiResult<Vec<ScannedFile>> {
        let mut files = Vec::new();
        for dir in dirs {
            self.scan_dir(Path::new(dir), &mut files)?;
        }
        Ok(files)
    }

    fn scan_dir(&self, dir: &Path, files: &mut Vec<ScannedFile>) -> LantaiResult<()> {
        if !dir.is_dir() {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir).map_err(|e| {
            LantaiError::Indexing(format!("Failed to read directory {}: {e}", dir.display()))
        })?;

        for entry in entries {
            let entry = entry
                .map_err(|e| LantaiError::Indexing(format!("Failed to read dir entry: {e}")))?;
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir(&path, files)?;
            } else if path.extension().is_some_and(|ext| ext == "md") {
                let content = std::fs::read_to_string(&path)?;
                let metadata = std::fs::metadata(&path)?;
                let modified_at = metadata
                    .modified()
                    .map(chrono::DateTime::<chrono::Utc>::from)
                    .unwrap_or_else(|_| chrono::Utc::now());

                files.push(ScannedFile {
                    path: path.to_string_lossy().to_string(),
                    content_hash: content_hash(&content),
                    modified_at,
                });
            }
        }

        Ok(())
    }
}
