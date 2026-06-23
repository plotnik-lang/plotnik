//! Plotnik compiler driver.
//!
//! Orchestrates the pass crates (parse, analyze-*, lower-*, emit-*) into the
//! `Query`/`QueryBuilder` pipeline. Pass internals are deliberately NOT
//! re-exported: a consumer that needs a specific pass depends on its crate
//! directly.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod emit;
pub mod query;
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

pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::diagnostics::*;
    pub use plotnik_compiler_diagnostics::*;
}
pub use plotnik_compiler_core::ir as bytecode;
pub use plotnik_compiler_diagnostics::source;

pub use plotnik_compiler_diagnostics::{
    Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span,
};
pub use query::{Query, QueryBuilder};
