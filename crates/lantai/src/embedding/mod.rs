pub mod mock;
#[cfg(feature = "openai")]
pub mod openai;
#[cfg(test)]
mod tests;
pub mod traits;

pub use mock::MockEmbedding;
#[cfg(feature = "openai")]
pub use openai::OpenAIEmbedding;
pub use traits::EmbeddingProvider;
