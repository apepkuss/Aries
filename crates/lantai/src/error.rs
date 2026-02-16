/// lantai 统一 Result 类型
pub type LantaiResult<T> = Result<T, LantaiError>;

/// lantai 错误类型
#[derive(Debug, thiserror::Error)]
pub enum LantaiError {
    #[error("Config error: {0}")]
    Config(String),

    #[error("Database error: {0}")]
    Database(String),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Indexing error: {0}")]
    Indexing(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
