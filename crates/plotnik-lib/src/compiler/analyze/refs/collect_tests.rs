use crate::compiler::analyze::refs::collect::{contains_ref, ref_names};

use crate::compiler::Query;

#[test]
fn collect_refs_deduplicates() {
    let q = Query::expect("Q = {(Foo) (Foo)}");
    let expr = q.symbol_table().body("Q").unwrap();
    let refs = ref_names(expr);
    assert_eq!(refs.len(), 1);
}

#[test]
fn contains_ref_positive() {
    let q = Query::expect("Q = (x (Foo))");
    let expr = q.symbol_table().body("Q").unwrap();
    assert!(contains_ref(expr, "Foo"));
}

#[test]
fn contains_ref_negative() {
    let q = Query::expect("Q = (x (Foo))");
    let expr = q.symbol_table().body("Q").unwrap();
    assert!(!contains_ref(expr, "Bar"));
}
