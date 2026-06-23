#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Syntactic parsing for the query language: lexer, grammar, and CST/AST.

pub use plotnik_compiler_diagnostics::{Error, Result};

pub mod parser;
pub use parser::*;
