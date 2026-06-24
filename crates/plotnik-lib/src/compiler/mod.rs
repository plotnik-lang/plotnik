//! Plotnik compiler driver.
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

#[cfg(test)]
mod analyze_tests;
#[cfg(test)]
mod parser_tests;

// `compile_tests` drives the lowering pipeline end-to-end; kept under a `compile`
// module path so its committed insta snapshots resolve unchanged.
#[cfg(test)]
mod compile {
    mod compile_tests;
}

pub use crate::compiler::emit::tables::EmitError;
pub use crate::compiler::diagnostics::source;

pub use crate::compiler::diagnostics::{
    Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span,
};
pub use query::{CheckedQuery, CompiledQuery, Query, QueryBuilder};
