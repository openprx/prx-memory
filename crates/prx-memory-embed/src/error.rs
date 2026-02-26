use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("provider returned invalid response: {0}")]
    InvalidResponse(String),

    #[error("provider API error: status={status}, body={body}")]
    Api { status: u16, body: String },
}
