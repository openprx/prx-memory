use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::config::OpenAiCompatibleConfig;
use crate::error::ProviderError;
use crate::traits::EmbeddingProvider;
use crate::types::{EmbeddingRequest, EmbeddingResponse, EmbeddingTask};

#[derive(Clone)]
pub struct OpenAiCompatibleEmbeddingProvider {
    config: OpenAiCompatibleConfig,
    client: Client,
}

impl OpenAiCompatibleEmbeddingProvider {
    pub fn new(config: OpenAiCompatibleConfig) -> Result<Self, ProviderError> {
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, client })
    }

    fn task_name(&self, task: Option<EmbeddingTask>) -> Option<&str> {
        match task {
            Some(EmbeddingTask::Query) => self.config.task_query.as_deref(),
            Some(EmbeddingTask::Passage) => self.config.task_passage.as_deref(),
            _ => None,
        }
    }

    fn endpoint(&self) -> String {
        format!(
            "{}/v1/embeddings",
            self.config.base_url.trim_end_matches('/')
        )
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for OpenAiCompatibleEmbeddingProvider {
    fn name(&self) -> &'static str {
        "openai-compatible"
    }

    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError> {
        if request.inputs.is_empty() {
            return Err(ProviderError::Config(
                "embedding input is empty".to_string(),
            ));
        }

        let mut payload = Map::new();
        payload.insert(
            "model".to_string(),
            Value::String(self.config.model.clone()),
        );

        if request.inputs.len() == 1 {
            payload.insert(
                "input".to_string(),
                Value::String(request.inputs[0].clone()),
            );
        } else {
            payload.insert(
                "input".to_string(),
                Value::Array(request.inputs.iter().cloned().map(Value::String).collect()),
            );
        }

        if let Some(dim) = request.dimensions {
            payload.insert("dimensions".to_string(), Value::Number(dim.into()));
        }

        if let Some(task) = self.task_name(request.task) {
            payload.insert("task".to_string(), Value::String(task.to_string()));
        }

        let normalized = request.normalized.or(self.config.normalized_default);
        if let Some(n) = normalized {
            payload.insert("normalized".to_string(), Value::Bool(n));
        }

        let res = self
            .client
            .post(self.endpoint())
            .bearer_auth(&self.config.api_key)
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status().as_u16();
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status, body });
        }

        let parsed: OpenAiEmbeddingResponse = res.json().await?;
        if parsed.data.is_empty() {
            return Err(ProviderError::InvalidResponse(
                "no embeddings in response".to_string(),
            ));
        }

        let mut data = parsed.data;
        data.sort_by_key(|it| it.index);
        let vectors = data.into_iter().map(|it| it.embedding).collect();

        Ok(EmbeddingResponse {
            provider: self.name().to_string(),
            model: parsed.model,
            vectors,
            usage_tokens: parsed.usage.and_then(|u| u.total_tokens),
        })
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiEmbeddingResponse {
    model: String,
    data: Vec<EmbeddingItem>,
    usage: Option<Usage>,
}

#[derive(Debug, Deserialize)]
struct EmbeddingItem {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Debug, Deserialize)]
struct Usage {
    total_tokens: Option<u64>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct OpenAiEmbeddingRequest<'a> {
    model: &'a str,
    input: &'a [String],
}
