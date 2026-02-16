use crate::error::{LantaiError, LantaiResult};

/// Embedding 提供者 trait
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Provider 名称（如 "openai"）
    fn name(&self) -> &str;
    /// 模型名称（如 "text-embedding-3-small"）
    fn model(&self) -> &str;
    /// 向量维度
    fn dimensions(&self) -> usize;
    /// 批量计算 embedding
    async fn embed_batch(&self, texts: &[&str]) -> LantaiResult<Vec<Vec<f32>>>;
    /// 单条计算 embedding（默认实现调用 embed_batch）
    async fn embed(&self, text: &str) -> LantaiResult<Vec<f32>> {
        let results = self.embed_batch(&[text]).await?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| LantaiError::Embedding("Empty embedding result".into()))
    }
    /// 单批次最大文本数
    fn max_batch_size(&self) -> usize {
        256
    }
}
