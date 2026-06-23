//! Plotnik compiler driver.
//!
//! Orchestrates the parser, analysis, lowering, emission, and typegen modules
//! into the `Query`/`QueryBuilder` pipeline. Pass internals stay under this
//! module; the crate root re-exports only the facade-level API.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]
// Former crate-local pass APIs remain module boundaries after the fold; not
// every re-export is used by the public facade.
#![allow(dead_code, unused_imports)]

pub mod core;
pub mod diagnostics;
pub mod parse;
pub mod typegen;

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

pub use crate::compiler::core::ir as bytecode;
pub use crate::compiler::diagnostics::source;

pub use crate::compiler::diagnostics::{
    Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span,
};
pub use query::{Query, QueryBuilder};
