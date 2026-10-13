use plotnik_compiler::GrammarIdentity;
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};

use crate::error::CliError;

pub fn from_bytes(bytes: &[u8], source: &str) -> Result<Grammar, CliError> {
    let json = std::str::from_utf8(bytes).map_err(|error| {
        CliError::fatal(format!("grammar '{}' is not valid UTF-8: {error}", source))
    })?;
    let raw = RawGrammar::from_json(json).map_err(|error| {
        CliError::fatal(format!("failed to parse grammar '{}': {error:?}", source))
    })?;
    let identity = GrammarIdentity::from_json_bytes(raw.name.clone(), bytes, source);
    Grammar::from_raw(&raw)
        .map(|grammar| grammar.with_identity(identity))
        .map_err(|error| {
            CliError::fatal(format!(
                "failed to load grammar metadata '{}': {error:?}",
                source
            ))
        })
}
