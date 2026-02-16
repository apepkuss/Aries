use sha2::{Digest, Sha256};

/// 分块结果
#[derive(Debug, Clone)]
pub struct Chunk {
    pub source_path: String,
    /// heading 层级路径，如 "# Guide > ## Setup > ### Install"
    pub heading_path: String,
    pub content: String,
    /// 1-based 起始行号
    pub start_line: usize,
    /// 1-based 结束行号（inclusive）
    pub end_line: usize,
    /// 内容的 SHA256 hash
    pub content_hash: String,
}

/// 文件扫描记录
#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: String,
    /// 文件完整内容的 SHA256 hash
    pub content_hash: String,
    pub modified_at: chrono::DateTime<chrono::Utc>,
}

/// 计算文本内容的 SHA256 hash
pub fn content_hash(text: &str) -> String {
    let hash = Sha256::digest(text.as_bytes());
    hex::encode(hash)
}

/// 生成 chunk 的复合 ID
/// 格式：SHA256("md:{source}:{start_line}:{end_line}:{content_hash}:{model}")[:16]
pub fn generate_composite_id(
    source_path: &str,
    start_line: usize,
    end_line: usize,
    content_hash: &str,
    embedding_model: &str,
) -> String {
    let input =
        format!("md:{source_path}:{start_line}:{end_line}:{content_hash}:{embedding_model}");
    let hash = Sha256::digest(input.as_bytes());
    hex::encode(&hash[..8]) // 16 hex chars = 8 bytes
}
