//! Utilities for working with definition references in expressions.

use indexmap::IndexSet;

use crate::parser::ast::{self, Expr};

/// Iterate over all Ref nodes in an expression tree.
pub fn ref_nodes(expr: &Expr) -> impl Iterator<Item = ast::Ref> + '_ {
    expr.as_cst().descendants().filter_map(ast::Ref::cast)
}

/// Collect all reference names as owned strings.
pub fn ref_names(expr: &Expr) -> IndexSet<String> {
    ref_nodes(expr)
        .filter_map(|r| r.name())
        .map(|tok| tok.text().to_string())
        .collect()
}

/// Check if expression contains a reference to the given name.
pub fn contains_ref(expr: &Expr, name: &str) -> bool {
    ref_nodes(expr)
        .filter_map(|r| r.name())
        .any(|tok| tok.text() == name)
}
