use super::Database;
use crate::error::{LantaiError, LantaiResult};

/// 文件索引记录
#[derive(Debug, Clone)]
pub struct FileRecord {
    pub path: String,
    pub content_hash: String,
    pub modified_at: String,
    pub indexed_at: String,
}

impl Database {
    /// 查询文件记录
    pub fn get_file(&self, path: &str) -> LantaiResult<Option<FileRecord>> {
        let mut stmt = self
            .conn()
            .prepare(
                "SELECT path, content_hash, modified_at, indexed_at FROM files WHERE path = ?1",
            )
            .map_err(|e| LantaiError::Database(format!("Failed to prepare get_file: {e}")))?;

        let result = stmt
            .query_row([path], |row| {
                Ok(FileRecord {
                    path: row.get(0)?,
                    content_hash: row.get(1)?,
                    modified_at: row.get(2)?,
                    indexed_at: row.get(3)?,
                })
            })
            .optional()
            .map_err(|e| LantaiError::Database(format!("Failed to query file: {e}")))?;

        Ok(result)
    }

    /// 插入或更新文件记录
    pub fn upsert_file(&self, record: &FileRecord) -> LantaiResult<()> {
        self.conn()
            .execute(
                "INSERT INTO files (path, content_hash, modified_at, indexed_at) \
                 VALUES (?1, ?2, ?3, ?4) \
                 ON CONFLICT(path) DO UPDATE SET \
                 content_hash = excluded.content_hash, \
                 modified_at = excluded.modified_at, \
                 indexed_at = excluded.indexed_at",
                rusqlite::params![
                    record.path,
                    record.content_hash,
                    record.modified_at,
                    record.indexed_at,
                ],
            )
            .map_err(|e| LantaiError::Database(format!("Failed to upsert file: {e}")))?;
        Ok(())
    }

    /// 删除文件记录（级联删除相关 chunks）
    pub fn delete_file(&self, path: &str) -> LantaiResult<()> {
        self.conn()
            .execute("DELETE FROM files WHERE path = ?1", [path])
            .map_err(|e| LantaiError::Database(format!("Failed to delete file: {e}")))?;
        Ok(())
    }

    /// 列出所有已索引的文件路径
    pub fn list_indexed_files(&self) -> LantaiResult<Vec<String>> {
        let mut stmt = self
            .conn()
            .prepare("SELECT path FROM files ORDER BY path")
            .map_err(|e| LantaiError::Database(format!("Failed to prepare list_files: {e}")))?;

        let paths = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| LantaiError::Database(format!("Failed to list files: {e}")))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|e| LantaiError::Database(format!("Failed to collect files: {e}")))?;

        Ok(paths)
    }
}

/// rusqlite Optional 查询辅助 trait
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
