use async_trait::async_trait;

use crate::error::ProviderError;
use crate::types::{EmbeddingRequest, EmbeddingResponse, RerankRequest, RerankResponse};

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError>;
}

#[async_trait]
pub trait RerankProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse, ProviderError>;
}
