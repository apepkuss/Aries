pub mod chunking;
pub mod config;
pub mod embedding;
pub mod error;
pub mod indexer;
pub mod search;
pub mod store;

pub use chunking::MarkdownChunker;
pub use config::LantaiConfig;
pub use embedding::{EmbeddingProvider, MockEmbedding};
pub use error::{LantaiError, LantaiResult};
pub use indexer::{IndexPipeline, IndexReport};
use search::HybridSearch;
pub use search::{SearchQuery, SearchResult};
pub use store::Database;

/// Lantai 主结构体 — 对外统一入口
pub struct Lantai {
    db: Database,
    chunker: MarkdownChunker,
    embedding: Box<dyn EmbeddingProvider>,
    config: LantaiConfig,
}

impl Lantai {
    /// 创建 Lantai 实例
    pub fn new(config: LantaiConfig, embedding: Box<dyn EmbeddingProvider>) -> LantaiResult<Self> {
        let db = Database::open(&config.database_path)?;

        // 确保 vec 表存在
        store::schema::ensure_vec_table(db.conn(), embedding.dimensions())?;

        let chunker = MarkdownChunker::new(
            config.chunking.max_chunk_lines,
            config.chunking.min_chunk_lines,
        );

        Ok(Self {
            db,
            chunker,
            embedding,
            config,
        })
    }

    /// 使用内存数据库创建（测试用）
    pub fn new_in_memory(
        config: LantaiConfig,
        embedding: Box<dyn EmbeddingProvider>,
    ) -> LantaiResult<Self> {
        let db = Database::open_in_memory()?;

        store::schema::ensure_vec_table(db.conn(), embedding.dimensions())?;

        let chunker = MarkdownChunker::new(
            config.chunking.max_chunk_lines,
            config.chunking.min_chunk_lines,
        );

        Ok(Self {
            db,
            chunker,
            embedding,
            config,
        })
    }

    /// 索引指定目录
    pub async fn index(&self, dirs: &[&str]) -> LantaiResult<IndexReport> {
        let pipeline = IndexPipeline::new(&self.db, &self.chunker, self.embedding.as_ref());
        pipeline.index_directories(dirs).await
    }

    /// 语义搜索（使用默认配置）
    pub async fn search(&self, query: &str) -> LantaiResult<Vec<SearchResult>> {
        let q = SearchQuery::new(query, self.config.search.default_limit);
        self.search_with_options(&q).await
    }

    /// 带选项的搜索
    pub async fn search_with_options(
        &self,
        query: &SearchQuery,
    ) -> LantaiResult<Vec<SearchResult>> {
        let hybrid = HybridSearch::new(&self.db, self.embedding.as_ref(), &self.config.search);
        hybrid.search(query).await
    }

    /// 获取索引统计
    pub fn stats(&self) -> LantaiResult<store::search::IndexStats> {
        self.db.get_stats()
    }
}
