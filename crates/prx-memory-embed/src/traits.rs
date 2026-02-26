use async_trait::async_trait;

use crate::error::ProviderError;
use crate::types::{EmbeddingRequest, EmbeddingResponse};

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError>;
}
