//! Plotnik compiler compatibility facade.
//!
//! The pipeline stages live in separate crates. This crate preserves the
//! historical `plotnik_compiler::{parser, analyze, compile, emit, query, ...}`
//! public surface for downstream callers.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub use plotnik_analyze as analyze;
pub use plotnik_compile as compile;
pub use plotnik_diagnostics as diagnostics;
pub use plotnik_diagnostics::source;
pub use plotnik_emit as emit;
pub use plotnik_ir as bytecode;
pub use plotnik_parser as parser;
pub use plotnik_query as query;
pub use plotnik_typegen as typegen;

pub use plotnik_diagnostics::{Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span};
pub use plotnik_query::{Query, QueryBuilder};
