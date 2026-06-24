#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod report;
pub mod source;
pub mod span;

#[cfg(test)]
mod source_tests;

pub use report::{DiagnosticKind, Diagnostics, Severity};
pub use source::{Source, SourceId, SourceKind, SourceMap, SourcePath};
pub use span::Span;

/// Errors that can occur during query parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Execution fuel exhausted (too many parser operations).
    #[error("execution limit exceeded")]
    ParseFuelExhausted,

    /// Recursion fuel exhausted (input nested too deeply).
    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,

    #[error("query parsing failed with {} errors", .0.error_count())]
    QueryParseError(Diagnostics),

    #[error("query analysis failed with {} errors", .0.error_count())]
    QueryAnalyzeError(Diagnostics),
}

/// Result type for query operations.
pub type QueryResult<T> = std::result::Result<T, Error>;
