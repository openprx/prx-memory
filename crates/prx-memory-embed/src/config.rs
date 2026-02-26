use std::time::Duration;

#[derive(Debug, Clone)]
pub struct OpenAiCompatibleConfig {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub timeout: Duration,
    pub task_query: Option<String>,
    pub task_passage: Option<String>,
    pub normalized_default: Option<bool>,
}

impl OpenAiCompatibleConfig {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com".to_string(),
            model: model.into(),
            timeout: Duration::from_secs(15),
            task_query: None,
            task_passage: None,
            normalized_default: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GeminiConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl GeminiConfig {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            model: model.into(),
            base_url: "https://generativelanguage.googleapis.com".to_string(),
            timeout: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone)]
pub enum EmbeddingProviderConfig {
    OpenAiCompatible(OpenAiCompatibleConfig),
    Jina(OpenAiCompatibleConfig),
    Gemini(GeminiConfig),
}
