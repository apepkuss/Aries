use rusqlite::Connection;

use crate::error::{LantaiError, LantaiResult};

const CURRENT_SCHEMA_VERSION: i64 = 1;

/// 核心 DDL — 在 schema 初始化时创建
const CORE_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS lantai_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS files (
    path         TEXT PRIMARY KEY,
    content_hash TEXT NOT NULL,
    modified_at  TEXT NOT NULL,
    indexed_at   TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS chunks (
    composite_id    TEXT PRIMARY KEY,
    source_path     TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    heading_path    TEXT NOT NULL DEFAULT '',
    content         TEXT NOT NULL,
    start_line      INTEGER NOT NULL,
    end_line        INTEGER NOT NULL,
    content_hash    TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    rowid_alias     INTEGER NOT NULL UNIQUE
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content,
    heading_path,
    content=chunks,
    content_rowid=rowid_alias
);

CREATE TRIGGER IF NOT EXISTS chunks_ai AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.rowid_alias, new.content, new.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_ad AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.rowid_alias, old.content, old.heading_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_au AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content, heading_path)
    VALUES ('delete', old.rowid_alias, old.content, old.heading_path);
    INSERT INTO chunks_fts(rowid, content, heading_path)
    VALUES (new.rowid_alias, new.content, new.heading_path);
END;

CREATE TABLE IF NOT EXISTS embedding_cache (
    content_hash    TEXT NOT NULL,
    embedding_model TEXT NOT NULL,
    embedding       BLOB NOT NULL,
    created_at      TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (content_hash, embedding_model)
);
"#;

/// 初始化数据库 schema（不含 chunks_vec，因维度需运行时确定）
pub fn initialize(conn: &Connection) -> LantaiResult<()> {
    let version = get_schema_version(conn);

    if version == 0 {
        // 全新数据库：创建所有表
        conn.execute_batch(CORE_DDL)
            .map_err(|e| LantaiError::Database(format!("Failed to initialize schema: {e}")))?;

        set_schema_version(conn, CURRENT_SCHEMA_VERSION)?;
    }
    // 未来版本迁移在此处添加：if version < 2 { migrate_v1_to_v2() }

    Ok(())
}

/// 创建 chunks_vec 向量表（维度在首次索引时确定）
pub fn ensure_vec_table(conn: &Connection, dimensions: usize) -> LantaiResult<()> {
    let sql = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(\
         chunk_rowid INTEGER PRIMARY KEY, \
         embedding float[{dimensions}] distance_metric=cosine\
         )"
    );
    conn.execute_batch(&sql)
        .map_err(|e| LantaiError::Database(format!("Failed to create chunks_vec: {e}")))?;

    // 记录 embedding 维度到 meta
    conn.execute(
        "INSERT OR REPLACE INTO lantai_meta (key, value) VALUES ('embedding_dimensions', ?1)",
        [&dimensions.to_string()],
    )
    .map_err(|e| LantaiError::Database(format!("Failed to store embedding dimensions: {e}")))?;

    Ok(())
}

/// 获取当前 schema 版本（0 表示未初始化）
fn get_schema_version(conn: &Connection) -> i64 {
    // 检查 lantai_meta 表是否存在
    let table_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='lantai_meta'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !table_exists {
        return 0;
    }

    conn.query_row(
        "SELECT value FROM lantai_meta WHERE key = 'schema_version'",
        [],
        |row| {
            let val: String = row.get(0)?;
            Ok(val.parse::<i64>().unwrap_or(0))
        },
    )
    .unwrap_or(0)
}

/// 设置 schema 版本
fn set_schema_version(conn: &Connection, version: i64) -> LantaiResult<()> {
    conn.execute(
        "INSERT OR REPLACE INTO lantai_meta (key, value) VALUES ('schema_version', ?1)",
        [&version.to_string()],
    )
    .map_err(|e| LantaiError::Database(format!("Failed to set schema version: {e}")))?;
    Ok(())
}
