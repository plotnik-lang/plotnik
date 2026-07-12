use crate::compiler::diagnostics::{Diagnostics, Error, QueryResult, SourceId};
use crate::compiler::parse::{ParseConfig, Root, parse_lossless};

use super::{model, render};

/// Format one Plotnik query file according to `docs/fmt.md`.
///
/// Successful output is canonical, idempotent, LF-only, and ends in exactly
/// one newline. Syntax diagnostics are returned as [`Error::QueryParseError`];
/// parser resource failures are propagated unchanged.
pub fn format_query(source: &str) -> QueryResult<String> {
    format_query_with_config(source, ParseConfig::default())
}

pub(super) fn format_query_with_config(source: &str, config: ParseConfig) -> QueryResult<String> {
    let root = parse(source, config)?;
    let file = model::normalize(source, &root);
    let mut output = render::render(&file);
    assert!(
        !output.ends_with('\n'),
        "the renderer returns a body without a terminal newline"
    );
    output.push('\n');
    Ok(output)
}

fn parse(source: &str, config: ParseConfig) -> QueryResult<Root> {
    let mut diagnostics = Diagnostics::new();
    let root = parse_lossless(source, SourceId::default(), &mut diagnostics, config)?;
    if diagnostics.has_errors() {
        return Err(Error::QueryParseError(diagnostics));
    }
    Ok(root)
}
