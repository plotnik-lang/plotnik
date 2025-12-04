use super::*;
use rowan::TextRange;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn error_stage_display() {
    insta::assert_snapshot!(format!("{}", DiagnosticStage::Parse), @"parse");
    insta::assert_snapshot!(format!("{}", DiagnosticStage::Validate), @"validate");
    insta::assert_snapshot!(format!("{}", DiagnosticStage::Resolve), @"resolve");
    insta::assert_snapshot!(format!("{}", DiagnosticStage::Escape), @"escape");
}

#[test]
fn diagnostic_warning_constructors() {
    let warn = DiagnosticMessage::warning(TextRange::empty(0.into()), "test warning");
    assert!(warn.is_warning());
    assert!(!warn.is_error());

    let warn_at = DiagnosticMessage::warning_at(5.into(), "warning at offset");
    assert!(warn_at.is_warning());
    assert_eq!(warn_at.range.start(), 5.into());
}

#[test]
fn diagnostic_error_at_constructor() {
    let err = DiagnosticMessage::error_at(7.into(), "error at offset");
    assert!(err.is_error());
    assert!(!err.is_warning());
    assert_eq!(err.range.start(), 7.into());
    assert_eq!(err.range.end(), 7.into());
}

#[test]
fn diagnostic_builders() {
    let diag = DiagnosticMessage::error(TextRange::empty(0.into()), "test")
        .with_stage(DiagnosticStage::Resolve)
        .with_fix(Fix::new("replacement", "description"))
        .with_related(RelatedInfo::new(TextRange::empty(10.into()), "related"));

    assert_eq!(diag.stage, DiagnosticStage::Resolve);
    assert!(diag.fix.is_some());
    assert_eq!(diag.related.len(), 1);

    let diag2 =
        DiagnosticMessage::error(TextRange::empty(0.into()), "test").with_related_many(vec![
            RelatedInfo::new(TextRange::empty(1.into()), "first"),
            RelatedInfo::new(TextRange::empty(2.into()), "second"),
        ]);
    assert_eq!(diag2.related.len(), 2);
}

#[test]
fn diagnostic_display() {
    let diag = DiagnosticMessage::error(TextRange::new(5.into(), 10.into()), "test message");
    insta::assert_snapshot!(format!("{}", diag), @"error at 5..10: test message");

    let diag_with_fix = DiagnosticMessage::error(TextRange::empty(0.into()), "msg")
        .with_fix(Fix::new("fix", "fix description"));
    insta::assert_snapshot!(format!("{}", diag_with_fix), @"error at 0..0: msg (fix: fix description)");

    let diag_with_related = DiagnosticMessage::error(TextRange::empty(0.into()), "msg")
        .with_related(RelatedInfo::new(TextRange::new(1.into(), 2.into()), "rel"));
    insta::assert_snapshot!(format!("{}", diag_with_related), @"error at 0..0: msg (related: rel at 1..2)");
}

#[test]
fn printer_colored() {
    let diag = DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "test");
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello").colored(true).render();
    assert!(result.contains("test"));
    assert!(result.contains('\x1b'));
}

#[test]
fn printer_empty_diagnostics() {
    let diagnostics = Diagnostics::new();
    let result = diagnostics.printer().source("source").render();
    assert!(result.is_empty());
}

#[test]
fn printer_with_path() {
    let diag = DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "test error");
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics
        .printer()
        .source("hello world")
        .path("test.pql")
        .render();
    insta::assert_snapshot!(result, @r"
    error: test error
     --> test.pql:1:1
      |
    1 | hello world
      | ^^^^^ test error
    ");
}

#[test]
fn printer_zero_width_span() {
    let diag = DiagnosticMessage::error(TextRange::empty(0.into()), "zero width error");
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello").render();
    insta::assert_snapshot!(result, @r"
    error: zero width error
      |
    1 | hello
      | ^ zero width error
    ");
}

#[test]
fn printer_with_related() {
    let diag =
        DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "primary").with_related(
            RelatedInfo::new(TextRange::new(6.into(), 10.into()), "related info"),
        );
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: primary
      |
    1 | hello world!
      | ^^^^^ ---- related info
      | |
      | primary
    ");
}

#[test]
fn printer_related_zero_width() {
    let diag =
        DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "primary").with_related(
            RelatedInfo::new(TextRange::empty(6.into()), "zero width related"),
        );
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: primary
      |
    1 | hello world!
      | ^^^^^ - zero width related
      | |
      | primary
    ");
}

#[test]
fn printer_with_fix() {
    let diag = DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "fixable")
        .with_fix(Fix::new("fixed", "apply this fix"));
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello world").render();
    insta::assert_snapshot!(result, @r"
    error: fixable
      |
    1 | hello world
      | ^^^^^ fixable
      |
    help: apply this fix
      |
    1 - hello world
    1 + fixed world
      |
    ");
}

#[test]
fn printer_multiple_diagnostics() {
    let diag1 = DiagnosticMessage::error(TextRange::new(0.into(), 5.into()), "first error");
    let diag2 = DiagnosticMessage::error(TextRange::new(6.into(), 10.into()), "second error");
    let diagnostics = Diagnostics::from_iter([diag1, diag2]);
    let result = diagnostics.printer().source("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: first error
      |
    1 | hello world!
      | ^^^^^ first error
    error: second error
      |
    1 | hello world!
      |       ^^^^ second error
    ");
}

#[test]
fn printer_warning() {
    let diag = DiagnosticMessage::warning(TextRange::new(0.into(), 5.into()), "a warning");
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().source("hello").render();
    insta::assert_snapshot!(result, @r"
    warning: a warning
      |
    1 | hello
      | ^^^^^ a warning
    ");
}

#[test]
fn printer_without_source_uses_plain_format() {
    let diag = DiagnosticMessage::error(TextRange::new(0.into(), 3.into()), "test");
    let diagnostics = Diagnostics::from_iter([diag]);
    let result = diagnostics.printer().render();
    insta::assert_snapshot!(result, @"error at 0..3: test");
}

#[test]
fn diagnostics_collection_methods() {
    let diag1 = DiagnosticMessage::error(TextRange::empty(0.into()), "error");
    let diag2 = DiagnosticMessage::warning(TextRange::empty(1.into()), "warning");
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(diag1);
    diagnostics.push(diag2);

    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.has_errors());
    assert!(diagnostics.has_warnings());
    assert_eq!(diagnostics.error_count(), 1);
    assert_eq!(diagnostics.warning_count(), 1);
    assert_eq!(diagnostics.filter_by_severity(Severity::Error).len(), 1);
    assert_eq!(diagnostics.filter_by_severity(Severity::Warning).len(), 1);
}

#[test]
fn diagnostics_iteration() {
    let diag1 = DiagnosticMessage::error(TextRange::empty(0.into()), "first");
    let diag2 = DiagnosticMessage::error(TextRange::empty(1.into()), "second");
    let diagnostics = Diagnostics::from_iter([diag1, diag2]);

    let messages: Vec<_> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(messages, vec!["first", "second"]);

    let messages: Vec<_> = (&diagnostics)
        .into_iter()
        .map(|d| d.message.as_str())
        .collect();
    assert_eq!(messages, vec!["first", "second"]);

    let vec = diagnostics.into_vec();
    assert_eq!(vec.len(), 2);
}
