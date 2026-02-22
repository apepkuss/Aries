use std::collections::HashMap;

use super::types::{SearchQuery, SearchResult};
use crate::{
    config::SearchConfig, embedding::EmbeddingProvider, error::LantaiResult, store::Database,
};

/// 混合搜索引擎（向量 + FTS5 + RRF 融合）
///
/// `embedding` 为 `None` 时退化为 BM25-only 搜索。
pub struct HybridSearch<'a> {
    db: &'a Database,
    embedding: Option<&'a dyn EmbeddingProvider>,
    config: &'a SearchConfig,
}

impl<'a> HybridSearch<'a> {
    pub fn new(
        db: &'a Database,
        embedding: Option<&'a dyn EmbeddingProvider>,
        config: &'a SearchConfig,
    ) -> Self {
        Self {
            db,
            embedding,
            config,
        }
    }

    /// 执行搜索
    ///
    /// 有 embedding 时使用混合搜索（向量 + BM25 + RRF 融合），否则使用 BM25-only。
    pub async fn search(&self, query: &SearchQuery) -> LantaiResult<Vec<SearchResult>> {
        let limit = query.limit;

        match self.embedding {
            Some(emb) => self.search_hybrid(emb, query, limit).await,
            None => self.search_bm25_only(query, limit),
        }
    }

    /// 混合搜索路径（向量 + BM25 + RRF 融合）
    async fn search_hybrid(
        &self,
        embedding: &dyn EmbeddingProvider,
        query: &SearchQuery,
        limit: usize,
    ) -> LantaiResult<Vec<SearchResult>> {
        let vec_weight = query.vec_weight.unwrap_or(self.config.vec_weight);
        let bm25_weight = query.bm25_weight.unwrap_or(self.config.bm25_weight);
        let rrf_k = self.config.rrf_k;

        // 1. 计算 query embedding
        let query_embedding = embedding.embed(&query.text).await?;

        // 2. 执行两路搜索（取较大的候选集）
        let candidate_limit = limit * 3;
        let vec_results = self.db.search_vec(&query_embedding, candidate_limit)?;
        let fts_results = self.db.search_fts(&query.text, candidate_limit)?;

        // 3. RRF 合并
        let merged = rrf_merge(&vec_results, &fts_results, vec_weight, bm25_weight, rrf_k);

        // 4. 取 top-k 并填充完整内容
        let top_ids: Vec<&str> = merged
            .iter()
            .take(limit)
            .map(|(id, _)| id.as_str())
            .collect();
        let chunks = self.db.get_chunks_by_ids(&top_ids)?;

        // 5. 按 merged 分数排序组装 SearchResult
        let score_map: HashMap<&str, f64> = merged
            .iter()
            .map(|(id, score)| (id.as_str(), *score))
            .collect();

        let mut results: Vec<SearchResult> = chunks
            .into_iter()
            .map(|c| SearchResult {
                score: score_map
                    .get(c.composite_id.as_str())
                    .copied()
                    .unwrap_or(0.0),
                chunk_id: c.composite_id,
                source_path: c.source_path,
                heading_path: c.heading_path,
                content: c.content,
                start_line: c.start_line as usize,
                end_line: c.end_line as usize,
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }

    /// BM25-only 搜索路径（无 embedding 时使用）
    fn search_bm25_only(
        &self,
        query: &SearchQuery,
        limit: usize,
    ) -> LantaiResult<Vec<SearchResult>> {
        let fts_results = self.db.search_fts(&query.text, limit)?;
        let top_ids: Vec<&str> = fts_results.iter().map(|(id, _)| id.as_str()).collect();
        let chunks = self.db.get_chunks_by_ids(&top_ids)?;

        let score_map: HashMap<&str, f64> = fts_results
            .iter()
            .map(|(id, score)| (id.as_str(), *score))
            .collect();

        let mut results: Vec<SearchResult> = chunks
            .into_iter()
            .map(|c| SearchResult {
                score: score_map
                    .get(c.composite_id.as_str())
                    .copied()
                    .unwrap_or(0.0),
                chunk_id: c.composite_id,
                source_path: c.source_path,
                heading_path: c.heading_path,
                content: c.content,
                start_line: c.start_line as usize,
                end_line: c.end_line as usize,
            })
            .collect();

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(results)
    }
}

/// RRF (Reciprocal Rank Fusion) 合并
///
/// score = vec_weight / (k + rank + 1) + bm25_weight / (k + rank + 1)
pub fn rrf_merge(
    vec_results: &[(String, f64)],
    fts_results: &[(String, f64)],
    vec_weight: f64,
    bm25_weight: f64,
    k: u32,
) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();

    for (rank, (id, _)) in vec_results.iter().enumerate() {
        *scores.entry(id.clone()).or_default() += vec_weight / (k as f64 + rank as f64 + 1.0);
    }

    for (rank, (id, _)) in fts_results.iter().enumerate() {
        *scores.entry(id.clone()).or_default() += bm25_weight / (k as f64 + rank as f64 + 1.0);
    }

    let mut results: Vec<_> = scores.into_iter().collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results
}
