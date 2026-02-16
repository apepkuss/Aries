use super::Database;
use crate::error::{LantaiError, LantaiResult};

/// chunk 数据库记录
#[derive(Debug, Clone)]
pub struct ChunkRecord {
    pub composite_id: String,
    pub source_path: String,
    pub heading_path: String,
    pub content: String,
    pub start_line: i64,
    pub end_line: i64,
    pub content_hash: String,
    pub embedding_model: String,
    pub embedding: Option<Vec<f32>>,
}

impl Database {
    /// 批量插入 chunks（含 embedding 写入 chunks_vec）
    pub fn insert_chunks(&self, chunks: &[ChunkRecord]) -> LantaiResult<()> {
        let tx = self
            .conn()
            .unchecked_transaction()
            .map_err(|e| LantaiError::Database(format!("Failed to begin transaction: {e}")))?;

        for chunk in chunks {
            let rowid = self.next_rowid();

            tx.execute(
                "INSERT INTO chunks \
                 (composite_id, source_path, heading_path, content, start_line, end_line, \
                  content_hash, embedding_model, rowid_alias) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    chunk.composite_id,
                    chunk.source_path,
                    chunk.heading_path,
                    chunk.content,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.content_hash,
                    chunk.embedding_model,
                    rowid,
                ],
            )
            .map_err(|e| {
                LantaiError::Database(format!(
                    "Failed to insert chunk {}: {e}",
                    chunk.composite_id
                ))
            })?;

            // 写入向量表（如果有 embedding）
            if let Some(ref embedding) = chunk.embedding {
                let blob: &[u8] = bytemuck::cast_slice(embedding);
                tx.execute(
                    "INSERT INTO chunks_vec (chunk_rowid, embedding) VALUES (?1, ?2)",
                    rusqlite::params![rowid, blob],
                )
                .map_err(|e| {
                    LantaiError::Database(format!(
                        "Failed to insert vec for chunk {}: {e}",
                        chunk.composite_id
                    ))
                })?;
            }
        }

        tx.commit()
            .map_err(|e| LantaiError::Database(format!("Failed to commit chunks: {e}")))?;

        Ok(())
    }

    /// 删除指定文件的所有 chunks（同时清理 chunks_vec）
    pub fn delete_chunks_by_file(&self, source_path: &str) -> LantaiResult<()> {
        let tx = self
            .conn()
            .unchecked_transaction()
            .map_err(|e| LantaiError::Database(format!("Failed to begin transaction: {e}")))?;

        // 先获取要删除的 rowid_alias 列表
        let rowids: Vec<i64> = {
            let mut stmt = tx
                .prepare("SELECT rowid_alias FROM chunks WHERE source_path = ?1")
                .map_err(|e| {
                    LantaiError::Database(format!("Failed to prepare delete query: {e}"))
                })?;

            stmt.query_map([source_path], |row| row.get(0))
                .map_err(|e| LantaiError::Database(format!("Failed to query chunk rowids: {e}")))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| LantaiError::Database(format!("Failed to collect rowids: {e}")))?
        };

        // 删除 chunks_vec 记录
        for rowid in &rowids {
            tx.execute("DELETE FROM chunks_vec WHERE chunk_rowid = ?1", [rowid])
                .ok(); // chunks_vec 可能不存在，忽略错误
        }

        // 删除 chunks（FTS5 trigger 会自动同步 chunks_fts）
        tx.execute("DELETE FROM chunks WHERE source_path = ?1", [source_path])
            .map_err(|e| LantaiError::Database(format!("Failed to delete chunks: {e}")))?;

        tx.commit()
            .map_err(|e| LantaiError::Database(format!("Failed to commit delete: {e}")))?;

        Ok(())
    }

    /// 查询指定文件的所有 chunk composite_id
    pub fn get_chunk_ids_by_file(&self, source_path: &str) -> LantaiResult<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT composite_id FROM chunks WHERE source_path = ?1")
            .map_err(|e| {
                LantaiError::Database(format!("Failed to prepare chunk_ids query: {e}"))
            })?;

        let ids = stmt
            .query_map([source_path], |row| row.get(0))
            .map_err(|e| LantaiError::Database(format!("Failed to query chunk_ids: {e}")))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| LantaiError::Database(format!("Failed to collect chunk_ids: {e}")))?;

        Ok(ids)
    }

    /// 查询 embedding 缓存
    pub fn get_cached_embedding(
        &self,
        content_hash: &str,
        model: &str,
    ) -> LantaiResult<Option<Vec<f32>>> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT embedding FROM embedding_cache \
                 WHERE content_hash = ?1 AND embedding_model = ?2",
            )
            .map_err(|e| LantaiError::Database(format!("Failed to prepare cache query: {e}")))?;

        let result = stmt
            .query_row(rusqlite::params![content_hash, model], |row| {
                let blob: Vec<u8> = row.get(0)?;
                let floats: Vec<f32> = bytemuck::cast_slice(&blob).to_vec();
                Ok(floats)
            })
            .optional()
            .map_err(|e| LantaiError::Database(format!("Failed to query cache: {e}")))?;

        Ok(result)
    }

    /// 写入 embedding 缓存
    pub fn cache_embedding(
        &self,
        content_hash: &str,
        model: &str,
        embedding: &[f32],
    ) -> LantaiResult<()> {
        let blob: &[u8] = bytemuck::cast_slice(embedding);
        self.conn()
            .execute(
                "INSERT OR REPLACE INTO embedding_cache \
                 (content_hash, embedding_model, embedding) VALUES (?1, ?2, ?3)",
                rusqlite::params![content_hash, model, blob],
            )
            .map_err(|e| LantaiError::Database(format!("Failed to cache embedding: {e}")))?;
        Ok(())
    }
}

/// rusqlite Optional 查询辅助
trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(val) => Ok(Some(val)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
