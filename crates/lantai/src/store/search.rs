use super::{Database, chunks::ChunkRecord};
use crate::error::{LantaiError, LantaiResult};

/// 索引统计信息
#[derive(Debug, Clone)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_chunks: usize,
    pub total_cached_embeddings: usize,
}

impl Database {
    /// FTS5 BM25 搜索，返回 (chunk_composite_id, bm25_score)
    pub fn search_fts(&self, query: &str, limit: usize) -> LantaiResult<Vec<(String, f64)>> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT c.composite_id, rank \
                 FROM chunks_fts f \
                 JOIN chunks c ON c.rowid_alias = f.rowid \
                 WHERE chunks_fts MATCH ?1 \
                 ORDER BY rank \
                 LIMIT ?2",
            )
            .map_err(|e| LantaiError::Database(format!("Failed to prepare FTS search: {e}")))?;

        let results = stmt
            .query_map(rusqlite::params![query, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| LantaiError::Database(format!("Failed to execute FTS search: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| LantaiError::Database(format!("Failed to collect FTS results: {e}")))?;

        Ok(results)
    }

    /// vec0 向量搜索，返回 (chunk_composite_id, distance)
    pub fn search_vec(&self, embedding: &[f32], limit: usize) -> LantaiResult<Vec<(String, f64)>> {
        let blob: &[u8] = bytemuck::cast_slice(embedding);

        let mut stmt = self
            .conn()
            .prepare(
                "SELECT c.composite_id, v.distance \
                 FROM chunks_vec v \
                 JOIN chunks c ON c.rowid_alias = v.chunk_rowid \
                 WHERE v.embedding MATCH ?1 AND k = ?2 \
                 ORDER BY v.distance",
            )
            .map_err(|e| LantaiError::Database(format!("Failed to prepare vec search: {e}")))?;

        let results = stmt
            .query_map(rusqlite::params![blob, limit as i64], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .map_err(|e| LantaiError::Database(format!("Failed to execute vec search: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| LantaiError::Database(format!("Failed to collect vec results: {e}")))?;

        Ok(results)
    }

    /// 按 composite_id 批量获取 chunk 完整内容
    pub fn get_chunks_by_ids(&self, ids: &[&str]) -> LantaiResult<Vec<ChunkRecord>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }

        let placeholders: String = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT composite_id, source_path, heading_path, content, \
             start_line, end_line, content_hash, embedding_model \
             FROM chunks WHERE composite_id IN ({placeholders})"
        );

        let mut stmt = self
            .conn()
            .prepare(&sql)
            .map_err(|e| LantaiError::Database(format!("Failed to prepare get_chunks: {e}")))?;

        let params: Vec<&dyn rusqlite::types::ToSql> = ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();

        let results = stmt
            .query_map(params.as_slice(), |row| {
                Ok(ChunkRecord {
                    composite_id: row.get(0)?,
                    source_path: row.get(1)?,
                    heading_path: row.get(2)?,
                    content: row.get(3)?,
                    start_line: row.get(4)?,
                    end_line: row.get(5)?,
                    content_hash: row.get(6)?,
                    embedding_model: row.get(7)?,
                    embedding: None,
                })
            })
            .map_err(|e| LantaiError::Database(format!("Failed to query chunks: {e}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| LantaiError::Database(format!("Failed to collect chunks: {e}")))?;

        Ok(results)
    }

    /// 获取索引统计信息
    pub fn get_stats(&self) -> LantaiResult<IndexStats> {
        let total_files: usize = self
            .conn()
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
            .map_err(|e| LantaiError::Database(format!("Failed to count files: {e}")))?;

        let total_chunks: usize = self
            .conn()
            .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
            .map_err(|e| LantaiError::Database(format!("Failed to count chunks: {e}")))?;

        let total_cached_embeddings: usize = self
            .conn()
            .query_row("SELECT COUNT(*) FROM embedding_cache", [], |row| row.get(0))
            .map_err(|e| LantaiError::Database(format!("Failed to count cache: {e}")))?;

        Ok(IndexStats {
            total_files,
            total_chunks,
            total_cached_embeddings,
        })
    }
}
