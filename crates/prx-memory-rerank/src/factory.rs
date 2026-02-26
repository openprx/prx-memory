use std::sync::Arc;

use crate::config::RerankProviderConfig;
use crate::error::ProviderError;
use crate::providers::{CohereRerankProvider, JinaRerankProvider, PineconeRerankProvider};
use crate::traits::RerankProvider;

pub fn build_rerank_provider(
    cfg: RerankProviderConfig,
) -> Result<Arc<dyn RerankProvider>, ProviderError> {
    match cfg {
        RerankProviderConfig::Jina(c) => Ok(Arc::new(JinaRerankProvider::new(c)?)),
        RerankProviderConfig::Cohere(c) => Ok(Arc::new(CohereRerankProvider::new(c)?)),
        RerankProviderConfig::Pinecone(c) => Ok(Arc::new(PineconeRerankProvider::new(c)?)),
    }
}
