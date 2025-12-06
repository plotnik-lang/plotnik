use super::*;

#[test]
fn valid_query() {
    let q = Query::try_from("Expr = (expression)").unwrap();
    assert!(q.is_valid());
}

#[test]
fn parse_error() {
    let q = Query::try_from("(unclosed").unwrap();
    assert!(!q.is_valid());
    assert!(q.dump_diagnostics().contains("expected"));
}

#[test]
fn resolution_error() {
    let q = Query::try_from("(call (Undefined))").unwrap();
    assert!(!q.is_valid());
    assert!(q.dump_diagnostics().contains("is not defined"));
}

#[test]
fn combined_errors() {
    let q = Query::try_from("(call (Undefined) extra)").unwrap();
    assert!(!q.is_valid());
    assert!(!q.diagnostics().is_empty());
}
