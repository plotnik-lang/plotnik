//! Plotnik compiler pipeline.
//!
//! Orchestrates the parser, analysis, lowering, emission, and typegen modules
//! into the `Query`/`QueryBuilder` pipeline. Pass internals stay under this
//! module; the crate root re-exports only the facade-level API.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub(crate) mod diagnostics;
mod ids;
pub(crate) mod parse;
pub(crate) mod typegen;

mod analyze;
mod lower;

pub(crate) mod emit;
pub(crate) mod query;
#[cfg(test)]
pub mod test_utils;

pub use crate::compiler::emit::tables::EmitError;
pub use crate::compiler::diagnostics::source;

pub use crate::compiler::diagnostics::{
    Diagnostics, Error, QueryResult, Severity, SourceId, SourceMap, SourcePath, Span,
};
pub use query::{CheckedQuery, CompiledQuery, Query, QueryBuilder};
