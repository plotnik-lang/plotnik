//! Grammar productions for the query language.
//!
//! This module implements all `parse_*` methods as an extension of `Parser`.
//! The grammar follows tree-sitter query syntax with extensions for named subqueries.

mod atoms;
mod expressions;
mod fields;
mod items;
mod structures;
mod utils;
mod validation;
