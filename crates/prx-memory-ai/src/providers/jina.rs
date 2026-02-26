use reqwest::Client;
use serde::Deserialize;

use crate::config::{JinaRerankConfig, OpenAiCompatibleConfig};
use crate::error::ProviderError;
use crate::providers::openai_compatible::OpenAiCompatibleEmbeddingProvider;
use crate::traits::{EmbeddingProvider, RerankProvider};
use crate::types::{
    EmbeddingRequest, EmbeddingResponse, RerankItem, RerankRequest, RerankResponse,
};

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

#[derive(Clone)]
pub struct JinaRerankProvider {
    config: JinaRerankConfig,
    client: Client,
}

impl JinaRerankProvider {
    pub fn new(config: JinaRerankConfig) -> Result<Self, ProviderError> {
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, client })
    }
}

#[async_trait::async_trait]
impl RerankProvider for JinaRerankProvider {
    fn name(&self) -> &'static str {
        "jina"
    }

    async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse, ProviderError> {
        if request.documents.is_empty() {
            return Err(ProviderError::Config(
                "rerank documents is empty".to_string(),
            ));
        }

        let payload = serde_json::json!({
            "model": self.config.model,
            "query": request.query,
            "documents": request.documents,
            "top_n": request.top_n.unwrap_or(10),
        });

        let res = self
            .client
            .post(&self.config.endpoint)
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status().as_u16();
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status, body });
        }

        let parsed: JinaRerankResponse = res.json().await?;
        let items = parsed
            .results
            .into_iter()
            .map(|it| RerankItem {
                index: it.index,
                score: it.relevance_score,
            })
            .collect();

        Ok(RerankResponse {
            provider: self.name().to_string(),
            model: self.config.model.clone(),
            items,
        })
    }
}

#[derive(Debug, Deserialize)]
struct JinaRerankResponse {
    results: Vec<JinaRerankItem>,
}

#[derive(Debug, Deserialize)]
struct JinaRerankItem {
    index: usize,
    relevance_score: f32,
}
