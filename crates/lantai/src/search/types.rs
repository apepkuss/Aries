/// 搜索查询
#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub limit: usize,
    /// 覆盖配置默认的向量搜索权重
    pub vec_weight: Option<f64>,
    /// 覆盖配置默认的 BM25 搜索权重
    pub bm25_weight: Option<f64>,
}

impl SearchQuery {
    pub fn new(text: impl Into<String>, limit: usize) -> Self {
        Self {
            text: text.into(),
            limit,
            vec_weight: None,
            bm25_weight: None,
        }
    }
}

/// 搜索结果
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub chunk_id: String,
    pub source_path: String,
    pub heading_path: String,
    pub content: String,
    pub score: f64,
    pub start_line: usize,
    pub end_line: usize,
}
