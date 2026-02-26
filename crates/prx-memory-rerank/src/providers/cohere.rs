use reqwest::Client;
use serde::Deserialize;

use crate::config::CohereRerankConfig;
use crate::error::ProviderError;
use crate::traits::RerankProvider;
use crate::types::{RerankItem, RerankRequest, RerankResponse};

#[derive(Clone)]
pub struct CohereRerankProvider {
    config: CohereRerankConfig,
    client: Client,
}

impl CohereRerankProvider {
    pub fn new(config: CohereRerankConfig) -> Result<Self, ProviderError> {
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, client })
    }
}

#[async_trait::async_trait]
impl RerankProvider for CohereRerankProvider {
    fn name(&self) -> &'static str {
        "cohere"
    }

    async fn rerank(&self, request: RerankRequest) -> Result<RerankResponse, ProviderError> {
        if request.documents.is_empty() {
            return Err(ProviderError::Config(
                "rerank documents is empty".to_string(),
            ));
        }

        let documents = request
            .documents
            .into_iter()
            .map(|text| serde_json::json!({"text": text}))
            .collect::<Vec<_>>();
        let payload = serde_json::json!({
            "model": self.config.model,
            "query": request.query,
            "documents": documents,
            "top_n": request.top_n.unwrap_or(10),
            "return_documents": false,
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

        let parsed: CohereRerankResponse = res.json().await?;
        if parsed.results.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "cohere rerank returned empty results".to_string(),
            ));
        }

        let items = parsed
            .results
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
struct CohereRerankResponse {
    results: Vec<CohereRerankItem>,
}

#[derive(Debug, Deserialize)]
struct CohereRerankItem {
    index: usize,
    #[serde(alias = "relevance_score", alias = "score")]
    score: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cohere_response_score_aliases_parse() {
        let v1 = r#"{"results":[{"index":1,"relevance_score":0.91}]}"#;
        let p1: CohereRerankResponse = serde_json::from_str(v1).expect("parse cohere v1");
        assert_eq!(p1.results[0].index, 1);
        assert!((p1.results[0].score - 0.91).abs() < 1e-6);

        let v2 = r#"{"results":[{"index":0,"score":0.77}]}"#;
        let p2: CohereRerankResponse = serde_json::from_str(v2).expect("parse cohere v2");
        assert_eq!(p2.results[0].index, 0);
        assert!((p2.results[0].score - 0.77).abs() < 1e-6);
    }
}
