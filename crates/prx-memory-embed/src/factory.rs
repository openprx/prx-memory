use std::sync::Arc;

use crate::config::EmbeddingProviderConfig;
use crate::error::ProviderError;
use crate::providers::{
    GeminiEmbeddingProvider, JinaEmbeddingProvider, OpenAiCompatibleEmbeddingProvider,
};
use crate::traits::EmbeddingProvider;

pub fn build_embedding_provider(
    cfg: EmbeddingProviderConfig,
) -> Result<Arc<dyn EmbeddingProvider>, ProviderError> {
    match cfg {
        EmbeddingProviderConfig::OpenAiCompatible(c) => {
            Ok(Arc::new(OpenAiCompatibleEmbeddingProvider::new(c)?))
        }
        EmbeddingProviderConfig::Jina(c) => Ok(Arc::new(JinaEmbeddingProvider::new(c)?)),
        EmbeddingProviderConfig::Gemini(c) => Ok(Arc::new(GeminiEmbeddingProvider::new(c)?)),
    }
}
