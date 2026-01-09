//! Binary serialization for grammars using postcard.

use super::json::GrammarError;
use super::types::Grammar;

impl Grammar {
    /// Deserialize grammar from binary format.
    pub fn from_binary(bytes: &[u8]) -> Result<Self, GrammarError> {
        postcard::from_bytes(bytes).map_err(GrammarError::Binary)
    }

    /// Serialize grammar to binary format.
    pub fn to_binary(&self) -> Vec<u8> {
        postcard::to_allocvec(self).expect("serialization should not fail")
    }
}
