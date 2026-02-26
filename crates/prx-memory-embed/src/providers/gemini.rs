use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::config::GeminiConfig;
use crate::error::ProviderError;
use crate::traits::EmbeddingProvider;
use crate::types::{EmbeddingRequest, EmbeddingResponse, EmbeddingTask};

#[derive(Clone)]
pub struct GeminiEmbeddingProvider {
    config: GeminiConfig,
    client: Client,
}

impl GeminiEmbeddingProvider {
    pub fn new(config: GeminiConfig) -> Result<Self, ProviderError> {
        let client = Client::builder().timeout(config.timeout).build()?;
        Ok(Self { config, client })
    }

    fn embed_content_url(&self) -> String {
        format!(
            "{}/v1beta/models/{}:embedContent?key={}",
            self.config.base_url.trim_end_matches('/'),
            self.config.model,
            self.config.api_key
        )
    }

    fn batch_embed_url(&self) -> String {
        format!(
            "{}/v1beta/models/{}:batchEmbedContents?key={}",
            self.config.base_url.trim_end_matches('/'),
            self.config.model,
            self.config.api_key
        )
    }

    fn map_task(task: Option<EmbeddingTask>) -> Option<&'static str> {
        match task {
            Some(EmbeddingTask::Query) => Some("RETRIEVAL_QUERY"),
            Some(EmbeddingTask::Passage) => Some("RETRIEVAL_DOCUMENT"),
            Some(EmbeddingTask::Similarity) => Some("SEMANTIC_SIMILARITY"),
            Some(EmbeddingTask::Classification) => Some("CLASSIFICATION"),
            Some(EmbeddingTask::Clustering) => Some("CLUSTERING"),
            Some(EmbeddingTask::QuestionAnswering) => Some("QUESTION_ANSWERING"),
            Some(EmbeddingTask::FactVerification) => Some("FACT_VERIFICATION"),
            Some(EmbeddingTask::CodeRetrievalQuery) => Some("CODE_RETRIEVAL_QUERY"),
            None => None,
        }
    }
}

#[async_trait::async_trait]
impl EmbeddingProvider for GeminiEmbeddingProvider {
    fn name(&self) -> &'static str {
        "gemini"
    }

    async fn embed(&self, request: EmbeddingRequest) -> Result<EmbeddingResponse, ProviderError> {
        if request.inputs.is_empty() {
            return Err(ProviderError::Config(
                "embedding input is empty".to_string(),
            ));
        }

        if request.inputs.len() == 1 {
            let payload = GeminiEmbedRequest {
                model: format!("models/{}", self.config.model),
                content: Content::from_text(&request.inputs[0]),
                task_type: Self::map_task(request.task).map(ToString::to_string),
                output_dimensionality: request.dimensions.map(|d| d as u32),
            };

            let res = self
                .client
                .post(self.embed_content_url())
                .json(&payload)
                .send()
                .await?;
            if !res.status().is_success() {
                let status = res.status().as_u16();
                let body = res.text().await.unwrap_or_default();
                return Err(ProviderError::Api { status, body });
            }

            let parsed: GeminiEmbedResponse = res.json().await?;
            return Ok(EmbeddingResponse {
                provider: self.name().to_string(),
                model: self.config.model.clone(),
                vectors: vec![parsed.embedding.values],
                usage_tokens: None,
            });
        }

        let requests: Vec<GeminiEmbedRequest> = request
            .inputs
            .iter()
            .map(|input| GeminiEmbedRequest {
                model: format!("models/{}", self.config.model),
                content: Content::from_text(input),
                task_type: Self::map_task(request.task).map(ToString::to_string),
                output_dimensionality: request.dimensions.map(|d| d as u32),
            })
            .collect();

        let payload = GeminiBatchEmbedRequest { requests };
        let res = self
            .client
            .post(self.batch_embed_url())
            .json(&payload)
            .send()
            .await?;

        if !res.status().is_success() {
            let status = res.status().as_u16();
            let body = res.text().await.unwrap_or_default();
            return Err(ProviderError::Api { status, body });
        }

        let parsed: GeminiBatchEmbedResponse = res.json().await?;
        let vectors = parsed.embeddings.into_iter().map(|e| e.values).collect();

        Ok(EmbeddingResponse {
            provider: self.name().to_string(),
            model: self.config.model.clone(),
            vectors,
            usage_tokens: None,
        })
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEmbedRequest {
    model: String,
    content: Content,
    #[serde(skip_serializing_if = "Option::is_none")]
    task_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<u32>,
}

#[derive(Debug, Serialize)]
struct GeminiBatchEmbedRequest {
    requests: Vec<GeminiEmbedRequest>,
}

#[derive(Debug, Serialize)]
struct Content {
    parts: Vec<Part>,
}

impl Content {
    fn from_text(text: &str) -> Self {
        Self {
            parts: vec![Part {
                text: text.to_string(),
            }],
        }
    }
}

#[derive(Debug, Serialize)]
struct Part {
    text: String,
}

#[derive(Debug, Deserialize)]
struct GeminiEmbedResponse {
    embedding: GeminiEmbedding,
}

#[derive(Debug, Deserialize)]
struct GeminiBatchEmbedResponse {
    embeddings: Vec<GeminiEmbedding>,
}

#[derive(Debug, Deserialize)]
struct GeminiEmbedding {
    values: Vec<f32>,
}
