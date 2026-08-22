//! Provenance for one exact `grammar.json` artifact.

use std::fmt::Write as _;

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
        let digest = Sha256::digest(grammar_json);
        let mut sha256 = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut sha256, "{byte:02x}").expect("writing to a String cannot fail");
        }

        Self {
            name: name.into(),
            sha256,
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
