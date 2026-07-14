//! Provenance for one exact `grammar.json` artifact.

use sha2::{Digest, Sha256};

/// Exact grammar artifact used to bind a query.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct GrammarIdentity {
    pub(crate) name: String,
    pub(crate) sha256: String,
    pub(crate) source: String,
}

impl GrammarIdentity {
    /// Build identity after `grammar_json` has been parsed and validated at the
    /// caller's outside boundary. The digest covers the exact input bytes, not
    /// a re-serialized grammar.
    pub fn from_json_bytes(
        name: impl Into<String>,
        grammar_json: &[u8],
        source: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            sha256: format!("{:x}", Sha256::digest(grammar_json)),
            source: source.into(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    pub fn source(&self) -> &str {
        &self.source
    }
}
