#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingTask {
    Query,
    Passage,
    Similarity,
    Classification,
    Clustering,
    QuestionAnswering,
    FactVerification,
    CodeRetrievalQuery,
}

#[derive(Debug, Clone)]
pub struct EmbeddingRequest {
    pub inputs: Vec<String>,
    pub task: Option<EmbeddingTask>,
    pub dimensions: Option<usize>,
    pub normalized: Option<bool>,
}

impl EmbeddingRequest {
    pub fn single(input: impl Into<String>) -> Self {
        Self {
            inputs: vec![input.into()],
            task: None,
            dimensions: None,
            normalized: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmbeddingResponse {
    pub provider: String,
    pub model: String,
    pub vectors: Vec<Vec<f32>>,
    pub usage_tokens: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RerankRequest {
    pub query: String,
    pub documents: Vec<String>,
    pub top_n: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct RerankItem {
    pub index: usize,
    pub score: f32,
}

#[derive(Debug, Clone)]
pub struct RerankResponse {
    pub provider: String,
    pub model: String,
    pub items: Vec<RerankItem>,
}
