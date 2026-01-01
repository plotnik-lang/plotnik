//! Utilities for working with definition references in expressions.

use indexmap::IndexSet;

use crate::parser::ast::{self, Expr};

/// Iterate over all Ref nodes in an expression tree.
pub fn ref_nodes(expr: &Expr) -> impl Iterator<Item = ast::Ref> + '_ {
    expr.as_cst().descendants().filter_map(ast::Ref::cast)
}

/// Collect all reference names as owned strings.
pub fn collect_ref_names(expr: &Expr) -> IndexSet<String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Query;

    #[test]
    fn collect_refs_from_simple_ref() {
        let q = Query::expect("Q = (Foo)");
        let expr = q.symbol_table.get("Q").unwrap();
        let refs = collect_ref_names(expr);
        assert_eq!(refs.len(), 1);
        assert!(refs.contains("Foo"));
    }

    #[test]
    fn collect_refs_from_nested() {
        let q = Query::expect("Q = (x (Foo) (Bar))");
        let expr = q.symbol_table.get("Q").unwrap();
        let refs = collect_ref_names(expr);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains("Foo"));
        assert!(refs.contains("Bar"));
    }

    #[test]
    fn collect_refs_deduplicates() {
        let q = Query::expect("Q = {(Foo) (Foo)}");
        let expr = q.symbol_table.get("Q").unwrap();
        let refs = collect_ref_names(expr);
        assert_eq!(refs.len(), 1);
    }

    #[test]
    fn contains_ref_positive() {
        let q = Query::expect("Q = (x (Foo))");
        let expr = q.symbol_table.get("Q").unwrap();
        assert!(contains_ref(expr, "Foo"));
    }

    #[test]
    fn contains_ref_negative() {
        let q = Query::expect("Q = (x (Foo))");
        let expr = q.symbol_table.get("Q").unwrap();
        assert!(!contains_ref(expr, "Bar"));
    }
}
