pub mod gemini;
pub mod jina;
pub mod openai_compatible;

pub use gemini::GeminiEmbeddingProvider;
pub use jina::JinaEmbeddingProvider;
pub use openai_compatible::OpenAiCompatibleEmbeddingProvider;
