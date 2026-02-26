use std::sync::Arc;

use crate::config::{EmbeddingProviderConfig, RerankProviderConfig};
use crate::error::ProviderError;
use crate::providers::{
    GeminiEmbeddingProvider, JinaEmbeddingProvider, JinaRerankProvider,
    OpenAiCompatibleEmbeddingProvider,
};
use crate::traits::{EmbeddingProvider, RerankProvider};

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

pub fn build_rerank_provider(
    cfg: RerankProviderConfig,
) -> Result<Arc<dyn RerankProvider>, ProviderError> {
    match cfg {
        RerankProviderConfig::Jina(c) => Ok(Arc::new(JinaRerankProvider::new(c)?)),
    }
}
