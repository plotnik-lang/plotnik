//! Grammar loading errors.

/// Error during grammar parsing or analysis.
#[derive(Debug)]
pub enum GrammarError {
    Json(serde_json::Error),
    Postcard(postcard::Error),
    Analysis(String),
}

impl std::fmt::Display for GrammarError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::Postcard(e) => write!(f, "postcard error: {e}"),
            Self::Analysis(e) => write!(f, "grammar analysis error: {e}"),
        }
    }
}

impl std::error::Error for GrammarError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            Self::Postcard(e) => Some(e),
            Self::Analysis(_) => None,
        }
    }
}
