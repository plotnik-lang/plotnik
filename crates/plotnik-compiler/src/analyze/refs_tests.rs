use super::refs::{contains_ref, ref_names};
use crate::Query;

#[test]
fn collect_refs_from_simple_ref() {
    let q = Query::expect("Q = (Foo)");
    let expr = q.symbol_table.get("Q").unwrap();
    let refs = ref_names(expr);
    assert_eq!(refs.len(), 1);
    assert!(refs.contains("Foo"));
}

#[test]
fn collect_refs_from_nested() {
    let q = Query::expect("Q = (x (Foo) (Bar))");
    let expr = q.symbol_table.get("Q").unwrap();
    let refs = ref_names(expr);
    assert_eq!(refs.len(), 2);
    assert!(refs.contains("Foo"));
    assert!(refs.contains("Bar"));
}

#[test]
fn collect_refs_deduplicates() {
    let q = Query::expect("Q = {(Foo) (Foo)}");
    let expr = q.symbol_table.get("Q").unwrap();
    let refs = ref_names(expr);
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
