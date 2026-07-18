use crate::compiler::diagnostics::{Diagnostics, Error, SourceId, SourceMap};
use crate::compiler::parse::{ParseConfig, Root, parse_lossless};

use super::{model, render};

pub type FormatResult<T> = Result<T, FormatError>;

/// A formatting failure with enough context to render syntax diagnostics.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FormatError {
    #[error("query parsing failed with {} errors", diagnostics.error_count())]
    Parse {
        diagnostics: Diagnostics,
        source_map: SourceMap,
    },

    #[error(transparent)]
    Resource(#[from] Error),
}

impl FormatError {
    pub fn diagnostics(&self) -> Option<&Diagnostics> {
        match self {
            Self::Parse { diagnostics, .. } => Some(diagnostics),
            Self::Resource(_) => None,
        }
    }

    pub fn source_map(&self) -> Option<&SourceMap> {
        match self {
            Self::Parse { source_map, .. } => Some(source_map),
            Self::Resource(_) => None,
        }
    }

    pub fn resource_error(&self) -> Option<&Error> {
        match self {
            Self::Resource(error) => Some(error),
            Self::Parse { .. } => None,
        }
    }
}

/// Format one Plotnik query file according to `docs/fmt.md`.
///
/// Successful output is canonical, idempotent, LF-only, and ends in exactly
/// one newline. Syntax failures carry their matching source map so callers can
/// render diagnostics without reconstructing formatter-internal context.
pub fn format_query(source: &str) -> FormatResult<String> {
    format_query_with_config(source, ParseConfig::default())
}

pub(super) fn format_query_with_config(source: &str, config: ParseConfig) -> FormatResult<String> {
    format_query_impl(source, config).map(|(output, _)| output)
}

fn format_query_impl(source: &str, config: ParseConfig) -> FormatResult<(String, usize)> {
    let root = parse(source, config)?;
    let file = model::normalize(source, &root);
    let rendered = render::render(&file);
    let mut output = rendered.output;
    assert!(
        !output.ends_with('\n'),
        "the renderer returns a body without a terminal newline"
    );
    output.push('\n');
    Ok((output, file.normalization_work + rendered.work))
}

fn parse(source: &str, config: ParseConfig) -> FormatResult<Root> {
    let mut diagnostics = Diagnostics::new();
    let root = parse_lossless(source, SourceId::default(), &mut diagnostics, config)
        .map_err(FormatError::Resource)?;
    if diagnostics.has_errors() {
        return Err(FormatError::Parse {
            diagnostics,
            source_map: SourceMap::from_inline(source),
        });
    }
    Ok(root)
}
