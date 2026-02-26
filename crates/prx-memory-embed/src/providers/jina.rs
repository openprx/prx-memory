use crate::config::OpenAiCompatibleConfig;
use crate::error::ProviderError;
use crate::providers::openai_compatible::OpenAiCompatibleEmbeddingProvider;
use crate::traits::EmbeddingProvider;
use crate::types::{EmbeddingRequest, EmbeddingResponse};

#[derive(Clone)]
pub struct JinaEmbeddingProvider {
    inner: OpenAiCompatibleEmbeddingProvider,
}

impl JinaEmbeddingProvider {
    pub fn new(mut config: OpenAiCompatibleConfig) -> Result<Self, ProviderError> {
        if config.base_url.trim().is_empty() {
            config.base_url = "https://api.jina.ai".to_string();
        }
        Ok(Self {
            inner: OpenAiCompatibleEmbeddingProvider::new(config)?,
        })
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for JinaEmbeddingProvider {
    fn name(&self) -> &'static str {
        "jina"
    }

    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError> {
        let mut res = self.inner.embed(request).await?;
        res.provider = self.name().to_string();
        Ok(res)
    }
}
