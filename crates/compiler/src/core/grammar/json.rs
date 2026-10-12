#[derive(Debug, thiserror::Error)]
pub enum GrammarError {
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("grammar analysis error: {0}")]
    Analysis(String),
}
