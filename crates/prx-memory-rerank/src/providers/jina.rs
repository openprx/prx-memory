use reqwest::Client;
use serde::Deserialize;

use crate::config::JinaRerankConfig;
use crate::error::ProviderError;
use crate::traits::RerankProvider;
use crate::types::{RerankItem, RerankRequest, RerankResponse};

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
