pub mod report;
pub mod source;
pub mod span;

#[cfg(test)]
mod source_tests;

pub use report::{DiagnosticBuilder, DiagnosticKind, Diagnostics, Severity};
pub use source::{Source, SourceId, SourceKind, SourceMap, SourcePath};
pub use span::Span;

/// Failures where the compiler could not answer a query operation.
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

    /// Emission configuration is outside-boundary input and has no honest
    /// query span.
    #[error("invalid emission configuration: {0}")]
    EmitConfig(#[from] crate::compiler::emit::EmitConfigError),

    /// Lowered compiler IR violated a contract shared by every executor.
    #[error("compiler invariant violation: {0}")]
    CompilerInvariantViolation(String),
}

/// Result type for query operations.
pub type QueryResult<T> = std::result::Result<T, Error>;
