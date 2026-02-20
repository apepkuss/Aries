use super::{chunks::ChunkRecord, connection::Database, files::FileRecord, schema};

/// 创建带 vec 表的测试数据库
fn test_db(dimensions: usize) -> Database {
    let db = Database::open_in_memory().unwrap();
    schema::ensure_vec_table(&db.conn(), dimensions).unwrap();
    db
}

#[test]
fn test_open_database() {
    let db = Database::open_in_memory().unwrap();

    // schema 已初始化：lantai_meta 应包含 schema_version
    let conn = db.conn();
    let version: String = conn
        .query_row(
            "SELECT value FROM lantai_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(version, "1");

    // files 表应存在
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // chunks 表应存在
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // sqlite-vec 扩展应已加载
    let vec_version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .unwrap();
    assert!(!vec_version.is_empty());
}

#[test]
fn test_file_crud() {
    let db = test_db(4);

    // 初始无文件
    let files = db.list_indexed_files().unwrap();
    assert!(files.is_empty());

    // 插入文件
    let record = FileRecord {
        path: "docs/test.md".to_string(),
        content_hash: "abc123".to_string(),
        modified_at: "2025-01-01T00:00:00Z".to_string(),
        indexed_at: "2025-01-01T00:00:01Z".to_string(),
    };
    db.upsert_file(&record).unwrap();

    // 查询文件
    let found = db.get_file("docs/test.md").unwrap().unwrap();
    assert_eq!(found.content_hash, "abc123");
    assert_eq!(found.modified_at, "2025-01-01T00:00:00Z");

    // 更新文件
    let updated = FileRecord {
        content_hash: "def456".to_string(),
        modified_at: "2025-01-02T00:00:00Z".to_string(),
        indexed_at: "2025-01-02T00:00:01Z".to_string(),
        ..record.clone()
    };
    db.upsert_file(&updated).unwrap();
    let found = db.get_file("docs/test.md").unwrap().unwrap();
    assert_eq!(found.content_hash, "def456");

    // 列出文件
    let files = db.list_indexed_files().unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0], "docs/test.md");

    // 查询不存在的文件
    let not_found = db.get_file("nonexistent.md").unwrap();
    assert!(not_found.is_none());

    // 删除文件
    db.delete_file("docs/test.md").unwrap();
    let files = db.list_indexed_files().unwrap();
    assert!(files.is_empty());
}

#[test]
fn test_chunk_crud() {
    let db = test_db(4);

    // 先插入 file 记录（FK 约束）
    db.upsert_file(&FileRecord {
        path: "docs/guide.md".to_string(),
        content_hash: "file_hash".to_string(),
        modified_at: "2025-01-01T00:00:00Z".to_string(),
        indexed_at: "2025-01-01T00:00:01Z".to_string(),
    })
    .unwrap();

    // 插入 chunks
    let chunks = vec![
        ChunkRecord {
            composite_id: "chunk_001".to_string(),
            source_path: "docs/guide.md".to_string(),
            heading_path: "# Guide".to_string(),
            content: "This is the introduction to the guide.".to_string(),
            start_line: 1,
            end_line: 5,
            content_hash: "hash_001".to_string(),
            embedding_model: "test-model".to_string(),
            embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
        },
        ChunkRecord {
            composite_id: "chunk_002".to_string(),
            source_path: "docs/guide.md".to_string(),
            heading_path: "# Guide > ## Setup".to_string(),
            content: "Follow these steps to set up the project.".to_string(),
            start_line: 6,
            end_line: 15,
            content_hash: "hash_002".to_string(),
            embedding_model: "test-model".to_string(),
            embedding: Some(vec![0.5, 0.6, 0.7, 0.8]),
        },
    ];

    db.insert_chunks(&chunks).unwrap();

    // 验证 chunk IDs
    let ids = db.get_chunk_ids_by_file("docs/guide.md").unwrap();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&"chunk_001".to_string()));
    assert!(ids.contains(&"chunk_002".to_string()));

    // 验证按 ID 获取
    let fetched = db.get_chunks_by_ids(&["chunk_001", "chunk_002"]).unwrap();
    assert_eq!(fetched.len(), 2);
    let c1 = fetched
        .iter()
        .find(|c| c.composite_id == "chunk_001")
        .unwrap();
    assert_eq!(c1.heading_path, "# Guide");
    assert_eq!(c1.content, "This is the introduction to the guide.");

    // 统计
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_files, 1);
    assert_eq!(stats.total_chunks, 2);

    // 删除文件的 chunks
    db.delete_chunks_by_file("docs/guide.md").unwrap();
    let ids = db.get_chunk_ids_by_file("docs/guide.md").unwrap();
    assert!(ids.is_empty());
}

#[test]
fn test_fts_search() {
    let db = test_db(4);

    db.upsert_file(&FileRecord {
        path: "docs/rust.md".to_string(),
        content_hash: "fts_hash".to_string(),
        modified_at: "2025-01-01T00:00:00Z".to_string(),
        indexed_at: "2025-01-01T00:00:01Z".to_string(),
    })
    .unwrap();

    let chunks = vec![
        ChunkRecord {
            composite_id: "fts_001".to_string(),
            source_path: "docs/rust.md".to_string(),
            heading_path: "# Rust Programming".to_string(),
            content: "Rust is a systems programming language focused on safety and performance."
                .to_string(),
            start_line: 1,
            end_line: 3,
            content_hash: "fts_h1".to_string(),
            embedding_model: "test".to_string(),
            embedding: Some(vec![0.1, 0.2, 0.3, 0.4]),
        },
        ChunkRecord {
            composite_id: "fts_002".to_string(),
            source_path: "docs/rust.md".to_string(),
            heading_path: "# Python Scripting".to_string(),
            content: "Python is a dynamic scripting language used for data science.".to_string(),
            start_line: 4,
            end_line: 6,
            content_hash: "fts_h2".to_string(),
            embedding_model: "test".to_string(),
            embedding: Some(vec![0.5, 0.6, 0.7, 0.8]),
        },
    ];

    db.insert_chunks(&chunks).unwrap();

    // 搜索 "Rust programming"
    let results = db.search_fts("Rust programming", 10).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].0, "fts_001");

    // 搜索 "Python"
    let results = db.search_fts("Python scripting", 10).unwrap();
    assert!(!results.is_empty());
    assert_eq!(results[0].0, "fts_002");

    // 搜索不存在的词
    let results = db.search_fts("nonexistent_xyz_term", 10).unwrap();
    assert!(results.is_empty());
}

#[test]
fn test_vec_search() {
    let db = test_db(4);

    db.upsert_file(&FileRecord {
        path: "docs/vec.md".to_string(),
        content_hash: "vec_hash".to_string(),
        modified_at: "2025-01-01T00:00:00Z".to_string(),
        indexed_at: "2025-01-01T00:00:01Z".to_string(),
    })
    .unwrap();

    let chunks = vec![
        ChunkRecord {
            composite_id: "vec_001".to_string(),
            source_path: "docs/vec.md".to_string(),
            heading_path: "# Topic A".to_string(),
            content: "Content about topic A.".to_string(),
            start_line: 1,
            end_line: 3,
            content_hash: "vh1".to_string(),
            embedding_model: "test".to_string(),
            embedding: Some(vec![1.0, 0.0, 0.0, 0.0]),
        },
        ChunkRecord {
            composite_id: "vec_002".to_string(),
            source_path: "docs/vec.md".to_string(),
            heading_path: "# Topic B".to_string(),
            content: "Content about topic B.".to_string(),
            start_line: 4,
            end_line: 6,
            content_hash: "vh2".to_string(),
            embedding_model: "test".to_string(),
            embedding: Some(vec![0.0, 1.0, 0.0, 0.0]),
        },
    ];

    db.insert_chunks(&chunks).unwrap();

    // 搜索与 vec_001 相似的向量
    let query = vec![0.9, 0.1, 0.0, 0.0];
    let results = db.search_vec(&query, 2).unwrap();
    assert_eq!(results.len(), 2);
    // 第一个结果应是 vec_001（最接近）
    assert_eq!(results[0].0, "vec_001");
    // distance 应小于第二个
    assert!(results[0].1 < results[1].1);

    // 搜索与 vec_002 相似的向量
    let query = vec![0.0, 0.9, 0.1, 0.0];
    let results = db.search_vec(&query, 2).unwrap();
    assert_eq!(results[0].0, "vec_002");
}

#[test]
fn test_embedding_cache() {
    let db = test_db(4);

    // 缓存未命中
    let cached = db.get_cached_embedding("hash_abc", "model-v1").unwrap();
    assert!(cached.is_none());

    // 写入缓存
    let embedding = vec![0.1, 0.2, 0.3, 0.4];
    db.cache_embedding("hash_abc", "model-v1", &embedding)
        .unwrap();

    // 缓存命中
    let cached = db
        .get_cached_embedding("hash_abc", "model-v1")
        .unwrap()
        .unwrap();
    assert_eq!(cached.len(), 4);
    assert!((cached[0] - 0.1).abs() < f32::EPSILON);
    assert!((cached[3] - 0.4).abs() < f32::EPSILON);

    // 不同 model 缓存未命中
    let cached = db.get_cached_embedding("hash_abc", "model-v2").unwrap();
    assert!(cached.is_none());

    // 覆盖写入
    let new_embedding = vec![0.5, 0.6, 0.7, 0.8];
    db.cache_embedding("hash_abc", "model-v1", &new_embedding)
        .unwrap();
    let cached = db
        .get_cached_embedding("hash_abc", "model-v1")
        .unwrap()
        .unwrap();
    assert!((cached[0] - 0.5).abs() < f32::EPSILON);

    // 统计
    let stats = db.get_stats().unwrap();
    assert_eq!(stats.total_cached_embeddings, 1);
}
