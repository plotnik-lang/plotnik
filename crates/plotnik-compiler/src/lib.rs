//! Plotnik compiler: parser, analyzer, and bytecode emitter.
//!
//! This crate provides the compilation pipeline for Plotnik queries:
//! - `parser` - lexer, CST, and AST construction
//! - `analyze` - semantic analysis (symbol table, type checking, validation)
//! - `compile` - Thompson NFA construction
//! - `emit` - bytecode emission
//! - `diagnostics` - error reporting
//! - `query` - high-level Query facade
//! - `typegen` - TypeScript type generation

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze;
pub mod bytecode;
pub mod compile;
pub mod diagnostics;
pub mod emit;
pub mod parser;
pub mod query;
pub mod typegen;

#[cfg(test)]
pub mod test_utils;

/// Result type for analysis passes that produce both output and diagnostics.
///
/// Each pass returns its typed output alongside any diagnostics it collected.
/// Fatal errors (like fuel exhaustion) use the outer `Result`.
pub type PassResult<T> = std::result::Result<(T, Diagnostics), Error>;

pub use diagnostics::{Diagnostics, DiagnosticsPrinter, Severity, Span};
pub use query::{Query, QueryBuilder};
pub use query::{SourceId, SourceMap};

/// Errors that can occur during query parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Execution fuel exhausted (too many parser operations).
    #[error("execution limit exceeded")]
    ExecFuelExhausted,

    /// Recursion fuel exhausted (input nested too deeply).
    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,

    #[error("query parsing failed with {} errors", .0.error_count())]
    QueryParseError(Diagnostics),

    #[error("query analysis failed with {} errors", .0.error_count())]
    QueryAnalyzeError(Diagnostics),
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, Error>;
