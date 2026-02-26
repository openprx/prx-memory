use reqwest::Client;
use serde::Deserialize;

use crate::config::PineconeRerankConfig;
use crate::error::ProviderError;
use crate::traits::RerankProvider;
use crate::types::{RerankItem, RerankRequest, RerankResponse};

#[derive(Clone)]
pub struct PineconeRerankProvider {
    config: PineconeRerankConfig,
    client: Client,
}

impl PineconeRerankProvider {
    pub fn new(config: PineconeRerankConfig) -> Result<Self, ProviderError> {
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, client })
    }
}

#[async_trait::async_trait]
impl RerankProvider for PineconeRerankProvider {
    fn name(&self) -> &'static str {
        "pinecone"
    }

    async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse, ProviderError> {
        if request.documents.is_empty() {
            return Err(ProviderError::Config(
                "rerank documents is empty".to_string(),
            ));
        }

        let top_n = request.top_n.unwrap_or(10);
        let payload = serde_json::json!({
            "model": self.config.model,
            "query": request.query,
            "documents": request.documents,
            "top_n": top_n,
            "topN": top_n,
            "return_documents": false,
        });

        let mut req = self
            .client
            .post(&self.config.endpoint)
            .header("Api-Key", &self.config.api_key)
            .json(&payload);
        if let Some(version) = &self.config.api_version {
            req = req.header("X-Pinecone-API-Version", version);
        }

        let res = req.send().await?;

        if !res.status().is_success() {
            let status = res.status().as_u16();
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status, body });
        }

        let parsed: PineconeRerankResponse = res.json().await?;
        let raw_items = if parsed.data.is_empty() {
            parsed.results
        } else {
            parsed.data
        };
        if raw_items.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "pinecone rerank returned empty results".to_string(),
            ));
        }

        let items = raw_items
            .into_iter()
            .map(|it| RerankItem {
                index: it.index,
                score: it.score,
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
struct PineconeRerankResponse {
    #[serde(default)]
    data: Vec<PineconeRerankItem>,
    #[serde(default)]
    results: Vec<PineconeRerankItem>,
}

#[derive(Debug, Deserialize)]
struct PineconeRerankItem {
    index: usize,
    #[serde(alias = "score", alias = "relevance_score")]
    score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinecone_response_variants_parse() {
        let v1 = r#"{"data":[{"index":2,"score":0.83}]}"#;
        let p1: PineconeRerankResponse = serde_json::from_str(v1).expect("parse pinecone data");
        assert_eq!(p1.data[0].index, 2);
        assert!((p1.data[0].score - 0.83).abs() < 1e-6);

        let v2 = r#"{"results":[{"index":0,"relevance_score":0.66}]}"#;
        let p2: PineconeRerankResponse = serde_json::from_str(v2).expect("parse pinecone results");
        assert_eq!(p2.results[0].index, 0);
        assert!((p2.results[0].score - 0.66).abs() < 1e-6);
    }
}
