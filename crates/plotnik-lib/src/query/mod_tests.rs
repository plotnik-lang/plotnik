use super::*;

#[test]
fn valid_query() {
    let q = Query::new("Expr = (expression)").unwrap();
    assert!(q.is_valid());
}

#[test]
fn parse_error() {
    let q = Query::new("(unclosed").unwrap();
    assert!(!q.is_valid());
    assert!(q.dump_diagnostics().contains("expected"));
}

#[test]
fn resolution_error() {
    let q = Query::new("(call (Undefined))").unwrap();
    assert!(!q.is_valid());
    assert!(q.dump_diagnostics().contains("undefined reference"));
}

#[test]
fn combined_errors() {
    let q = Query::new("(call (Undefined) extra)").unwrap();
    assert!(!q.is_valid());
    assert!(!q.diagnostics().is_empty());
}
