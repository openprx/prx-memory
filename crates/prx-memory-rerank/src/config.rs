use std::time::Duration;

#[derive(Debug, Clone)]
pub struct JinaRerankConfig {
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub timeout: Duration,
}

impl JinaRerankConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "jina-reranker-v2-base-multilingual".to_string(),
            endpoint: "https://api.jina.ai/v1/rerank".to_string(),
            timeout: Duration::from_secs(8),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CohereRerankConfig {
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub timeout: Duration,
}

impl CohereRerankConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "rerank-v3.5".to_string(),
            endpoint: "https://api.cohere.com/v2/rerank".to_string(),
            timeout: Duration::from_secs(8),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PineconeRerankConfig {
    pub api_key: String,
    pub model: String,
    pub endpoint: String,
    pub timeout: Duration,
    pub api_version: Option<String>,
}

impl PineconeRerankConfig {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: "bge-reranker-v2-m3".to_string(),
            endpoint: "https://api.pinecone.io/rerank".to_string(),
            timeout: Duration::from_secs(8),
            api_version: Some("2025-10".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum RerankProviderConfig {
    Jina(JinaRerankConfig),
    Cohere(CohereRerankConfig),
    Pinecone(PineconeRerankConfig),
}
