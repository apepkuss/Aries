pub mod chunking;
pub mod config;
pub mod embedding;
pub mod error;
pub mod indexer;
pub mod search;
pub mod store;
pub mod watcher;
pub mod writer;

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
    embedding: Option<Box<dyn EmbeddingProvider>>,
    config: LantaiConfig,
}

impl Lantai {
    /// 创建 Lantai 实例
    ///
    /// `embedding` 为 `None` 时以 BM25-only 模式运行：
    /// 索引时仅写入 FTS5，搜索时仅使用 BM25 全文检索。
    pub fn new(
        config: LantaiConfig,
        embedding: Option<Box<dyn EmbeddingProvider>>,
    ) -> LantaiResult<Self> {
        let db = Database::open(&config.database_path)?;

        // 仅当有 embedding 时才创建 vec 表
        if let Some(ref emb) = embedding {
            store::schema::ensure_vec_table(&db.conn(), emb.dimensions())?;
        }

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
        embedding: Option<Box<dyn EmbeddingProvider>>,
    ) -> LantaiResult<Self> {
        let db = Database::open_in_memory()?;

        if let Some(ref emb) = embedding {
            store::schema::ensure_vec_table(&db.conn(), emb.dimensions())?;
        }

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
        let pipeline = IndexPipeline::new(&self.db, &self.chunker, self.embedding.as_deref());
        pipeline.index_directories(dirs).await
    }

    /// 语义搜索（使用默认配置）
    ///
    /// 有 embedding 时使用混合搜索（向量 + BM25），否则使用 BM25-only。
    pub async fn search(&self, query: &str) -> LantaiResult<Vec<SearchResult>> {
        let q = SearchQuery::new(query, self.config.search.default_limit);
        self.search_with_options(&q).await
    }

    /// 带选项的搜索
    pub async fn search_with_options(
        &self,
        query: &SearchQuery,
    ) -> LantaiResult<Vec<SearchResult>> {
        let hybrid = HybridSearch::new(&self.db, self.embedding.as_deref(), &self.config.search);
        hybrid.search(query).await
    }

    /// 是否有 embedding provider（false 表示 BM25-only 模式）
    pub fn has_embedding(&self) -> bool {
        self.embedding.is_some()
    }

    /// 热替换 embedding provider
    ///
    /// 替换后：
    /// - 新的搜索和索引操作会使用新 provider
    /// - 如果新 provider 的维度与旧的不同，会重建 `chunks_vec` 向量表
    /// - 设为 `None` 会退化为 BM25-only 模式
    pub fn replace_embedding(
        &mut self,
        new_embedding: Option<Box<dyn EmbeddingProvider>>,
    ) -> LantaiResult<()> {
        if let Some(ref emb) = new_embedding {
            store::schema::ensure_vec_table(&self.db.conn(), emb.dimensions())?;
        }
        self.embedding = new_embedding;
        Ok(())
    }

    /// 获取索引统计
    pub fn stats(&self) -> LantaiResult<store::search::IndexStats> {
        self.db.get_stats()
    }
}
