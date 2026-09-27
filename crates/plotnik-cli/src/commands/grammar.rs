use std::fs;
use std::path::Path;

use plotnik_lib::GrammarIdentity;
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};

use crate::error::CliError;

pub fn load(path: &Path) -> Result<Grammar, CliError> {
    let bytes = fs::read(path).map_err(|error| {
        CliError::fatal(format!(
            "failed to read grammar '{}': {error}",
            path.display()
        ))
    })?;
    let json = std::str::from_utf8(&bytes).map_err(|error| {
        CliError::fatal(format!(
            "grammar '{}' is not valid UTF-8: {error}",
            path.display()
        ))
    })?;
    let raw = RawGrammar::from_json(json).map_err(|error| {
        CliError::fatal(format!(
            "failed to parse grammar '{}': {error:?}",
            path.display()
        ))
    })?;
    let identity =
        GrammarIdentity::from_json_bytes(raw.name.clone(), &bytes, path.display().to_string());
    Grammar::from_raw(&raw)
        .map(|grammar| grammar.with_identity(identity))
        .map_err(|error| {
            CliError::fatal(format!(
                "failed to load grammar metadata '{}': {error:?}",
                path.display()
            ))
        })
}
