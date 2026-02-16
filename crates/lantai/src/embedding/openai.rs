use super::traits::EmbeddingProvider;
use crate::error::{LantaiError, LantaiResult};

/// OpenAI 兼容 Embedding 实现
///
/// 兼容所有 OpenAI API 兼容服务（OpenAI、LlamaEdge、Ollama 等）
pub struct OpenAIEmbedding {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
    dimensions: usize,
}

impl OpenAIEmbedding {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        dimensions: usize,
    ) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            client: reqwest::Client::new(),
            base_url,
            api_key: api_key.into(),
            model: model.into(),
            dimensions,
        }
    }
}

/// OpenAI /embeddings 请求体
#[derive(serde::Serialize)]
struct EmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [&'a str],
}

/// OpenAI /embeddings 响应体
#[derive(serde::Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingData>,
}

#[derive(serde::Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAIEmbedding {
    fn name(&self) -> &str {
        "openai"
    }

    fn model(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }

    async fn embed_batch(&self, texts: &[&str]) -> LantaiResult<Vec<Vec<f32>>> {
        let mut all_embeddings = Vec::with_capacity(texts.len());
        let batch_size = self.max_batch_size();

        for batch_start in (0..texts.len()).step_by(batch_size) {
            let batch_end = (batch_start + batch_size).min(texts.len());
            let batch = &texts[batch_start..batch_end];

            let url = format!("{}/embeddings", self.base_url);
            let body = EmbeddingRequest {
                model: &self.model,
                input: batch,
            };

            let resp = self
                .client
                .post(&url)
                .bearer_auth(&self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| LantaiError::Embedding(format!("HTTP request failed: {e}")))?;

            let status = resp.status();
            if !status.is_success() {
                let body_text = resp.text().await.unwrap_or_default();
                return Err(LantaiError::Embedding(format!(
                    "API returned {status}: {body_text}"
                )));
            }

            let data: EmbeddingResponse = resp
                .json()
                .await
                .map_err(|e| LantaiError::Embedding(format!("Failed to parse response: {e}")))?;

            for item in data.data {
                all_embeddings.push(item.embedding);
            }
        }

        Ok(all_embeddings)
    }
}
