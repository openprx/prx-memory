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
