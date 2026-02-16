use sha2::{Digest, Sha256};

use super::traits::EmbeddingProvider;
use crate::error::LantaiResult;

/// 测试用 Mock Provider，基于文本 hash 生成确定性向量
pub struct MockEmbedding {
    dimensions: usize,
}

impl MockEmbedding {
    pub fn new(dimensions: usize) -> Self {
        Self { dimensions }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for MockEmbedding {
    fn name(&self) -> &str {
        "mock"
    }

    fn model(&self) -> &str {
        "mock-embedding"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_batch(&self, texts: &[&str]) -> LantaiResult<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| deterministic_vec(text, self.dimensions))
            .collect())
    }
}

/// 基于文本 SHA256 生成确定性浮点向量
///
/// 通过反复 hash 填充所需维度，每个分量归一化到 [-1, 1]
fn deterministic_vec(text: &str, dimensions: usize) -> Vec<f32> {
    let mut result = Vec::with_capacity(dimensions);
    let mut hasher_input = text.as_bytes().to_vec();

    while result.len() < dimensions {
        let hash = Sha256::digest(&hasher_input);
        // 每 4 字节转一个 f32，一次 hash 产生 8 个分量
        for chunk in hash.chunks(4) {
            if result.len() >= dimensions {
                break;
            }
            let bytes: [u8; 4] = chunk.try_into().unwrap();
            let val = u32::from_le_bytes(bytes);
            // 归一化到 [-1, 1]
            let f = (val as f64 / u32::MAX as f64) * 2.0 - 1.0;
            result.push(f as f32);
        }
        // 下一轮使用上一轮 hash 作为输入
        hasher_input = hash.to_vec();
    }

    result
}
