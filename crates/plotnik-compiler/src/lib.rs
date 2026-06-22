//! Plotnik compiler compatibility facade.
//!
//! The pipeline stages live in separate crates. This crate preserves the
//! historical `plotnik_compiler::{parser, analyze, compile, emit, query, ...}`
//! public surface for downstream callers.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(test)]
pub mod analyze;
#[cfg(test)]
pub mod bytecode;
#[cfg(test)]
pub mod compile;
#[cfg(test)]
pub mod diagnostics;
#[cfg(test)]
pub mod emit;
#[cfg(test)]
pub mod parser;
#[cfg(test)]
pub mod query;
#[cfg(test)]
pub mod source;
#[cfg(test)]
pub mod test_utils;
#[cfg(test)]
pub mod typegen;

#[cfg(not(test))]
pub use plotnik_analyze as analyze;
#[cfg(not(test))]
pub use plotnik_compile as compile;
#[cfg(not(test))]
pub use plotnik_diagnostics as diagnostics;
#[cfg(not(test))]
pub use plotnik_diagnostics::source;
#[cfg(not(test))]
pub use plotnik_emit as emit;
#[cfg(not(test))]
pub use plotnik_ir as bytecode;
#[cfg(not(test))]
pub use plotnik_parser as parser;
#[cfg(not(test))]
pub use plotnik_query as query;
#[cfg(not(test))]
pub use plotnik_typegen as typegen;

#[cfg(test)]
pub type PassResult<T> = std::result::Result<(T, Diagnostics), Error>;

#[cfg(test)]
pub use diagnostics::{Diagnostics, Severity, Span};
#[cfg(test)]
pub use query::{Query, QueryBuilder};
#[cfg(test)]
pub use source::{SourceId, SourceMap};

#[cfg(test)]
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

#[cfg(test)]
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(not(test))]
pub use plotnik_diagnostics::{Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span};
#[cfg(not(test))]
pub use plotnik_query::{Query, QueryBuilder};
