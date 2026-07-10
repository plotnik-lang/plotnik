//! Plotnik compiler pipeline.
//!
//! Orchestrates the parser, analysis, lowering, emission, and typegen modules
//! into the `Query`/`QueryBuilder` pipeline. Pass internals stay under this
//! module; the crate root re-exports only the facade-level API.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub(crate) mod codegen;
pub(crate) mod diagnostics;
mod ids;
pub(crate) mod limits;
pub(crate) mod parse;
pub(crate) mod regex;
pub(crate) mod srcgen;
pub(crate) mod typegen;

mod analyze;
mod lower;

#[cfg(test)]
mod regex_tests;

pub(crate) mod emit;
pub(crate) mod query;
#[cfg(test)]
pub mod test_utils;

pub use crate::compiler::diagnostics::source;

pub use crate::compiler::diagnostics::{
    DiagnosticBuilder, DiagnosticKind, Diagnostics, Error, QueryResult, Severity, Source, SourceId,
    SourceKind, SourceMap, SourcePath, Span,
};
pub use parse::{TokenSpan, tokenize};
pub use query::{CheckedQuery, CompiledQuery, Query, QueryBuilder};
