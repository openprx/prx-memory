use async_trait::async_trait;

use crate::error::ProviderError;
use crate::types::{RerankRequest, RerankResponse};

#[async_trait]
pub trait RerankProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse, ProviderError>;
}
