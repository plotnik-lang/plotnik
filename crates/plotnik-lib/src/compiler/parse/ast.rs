//! Typed AST wrappers now live in `compiler::core`; re-exported here so the
//! parser and its consumers keep referring to them as `crate::compiler::parse::ast::*`.

pub use crate::compiler::core::ast::*;
