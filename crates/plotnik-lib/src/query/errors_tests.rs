use super::Query;
use crate::parser::{ErrorStage, RenderOptions, Severity};

#[test]
fn diagnostics_alias() {
    let q = Query::new("(valid)").unwrap();
    assert_eq!(q.diagnostics().len(), q.errors().len());
}

#[test]
fn error_stage_filtering() {
    let q = Query::new("(unclosed").unwrap();
    assert!(q.has_parse_errors());
    assert!(!q.has_resolve_errors());
    assert!(!q.has_escape_errors());
    assert_eq!(q.errors_for_stage(ErrorStage::Parse).len(), 1);

    let q = Query::new("(call (Undefined))").unwrap();
    assert!(!q.has_parse_errors());
    assert!(q.has_resolve_errors());
    assert!(!q.has_escape_errors());
    assert_eq!(q.errors_for_stage(ErrorStage::Resolve).len(), 1);

    let q = Query::new("[A: (a) (b)]").unwrap();
    assert!(!q.has_parse_errors());
    assert!(q.has_validate_errors());
    assert!(!q.has_resolve_errors());
    assert!(!q.has_escape_errors());
    assert_eq!(q.errors_for_stage(ErrorStage::Validate).len(), 1);

    let q = Query::new("Expr = (call (Expr))").unwrap();
    assert!(!q.has_parse_errors());
    assert!(!q.has_validate_errors());
    assert!(!q.has_resolve_errors());
    assert!(q.has_escape_errors());
    assert_eq!(q.errors_for_stage(ErrorStage::Escape).len(), 1);

    let q = Query::new("Expr = (call (Expr)) (unclosed").unwrap();
    assert!(q.has_parse_errors());
    assert!(!q.has_resolve_errors());
    assert!(q.has_escape_errors());
}

#[test]
fn is_valid_ignores_warnings() {
    let q = Query::new("(valid)").unwrap();
    assert!(q.is_valid());
    assert!(!q.has_errors());
    assert!(!q.has_warnings());
    assert_eq!(q.error_count(), 0);
    assert_eq!(q.warning_count(), 0);
}

#[test]
fn error_and_warning_counts() {
    let q = Query::new("(unclosed").unwrap();
    assert!(q.has_errors());
    assert!(!q.has_warnings());
    assert_eq!(q.error_count(), 1);
    assert_eq!(q.warning_count(), 0);
}

#[test]
fn errors_only_and_warnings_only() {
    let q = Query::new("(unclosed").unwrap();
    let errors = q.errors_only();
    let warnings = q.warnings_only();
    assert_eq!(errors.len(), 1);
    assert!(warnings.is_empty());
}

#[test]
fn render_diagnostics_method() {
    let q = Query::new("(unclosed").unwrap();
    let rendered = q.render_diagnostics(RenderOptions::plain());
    insta::assert_snapshot!(rendered, @r"
    error: unclosed tree; expected ')'
      |
    1 | (unclosed
      | -        ^ unclosed tree; expected ')'
      | |
      | tree started here
    ");
}

#[test]
fn filter_by_severity() {
    let q = Query::new("(unclosed").unwrap();
    let errors = q.filter_by_severity(Severity::Error);
    let warnings = q.filter_by_severity(Severity::Warning);
    assert_eq!(errors.len(), 1);
    assert!(warnings.is_empty());
}
