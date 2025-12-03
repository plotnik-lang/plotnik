use super::*;
use rowan::TextRange;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn error_stage_display() {
    insta::assert_snapshot!(format!("{}", ErrorStage::Parse), @"parse");
    insta::assert_snapshot!(format!("{}", ErrorStage::Validate), @"validate");
    insta::assert_snapshot!(format!("{}", ErrorStage::Resolve), @"resolve");
    insta::assert_snapshot!(format!("{}", ErrorStage::Escape), @"escape");
}

#[test]
fn diagnostic_warning_constructors() {
    let warn = Diagnostic::warning(TextRange::empty(0.into()), "test warning");
    assert!(warn.is_warning());
    assert!(!warn.is_error());

    let warn_at = Diagnostic::warning_at(5.into(), "warning at offset");
    assert!(warn_at.is_warning());
    assert_eq!(warn_at.range.start(), 5.into());
}

#[test]
fn diagnostic_error_at_constructor() {
    let err = Diagnostic::error_at(7.into(), "error at offset");
    assert!(err.is_error());
    assert!(!err.is_warning());
    assert_eq!(err.range.start(), 7.into());
    assert_eq!(err.range.end(), 7.into());
}

#[test]
fn diagnostic_builders() {
    let diag = Diagnostic::error(TextRange::empty(0.into()), "test")
        .with_stage(ErrorStage::Resolve)
        .with_fix(Fix::new("replacement", "description"))
        .with_related(RelatedInfo::new(TextRange::empty(10.into()), "related"));

    assert_eq!(diag.stage, ErrorStage::Resolve);
    assert!(diag.fix.is_some());
    assert_eq!(diag.related.len(), 1);

    let diag2 = Diagnostic::error(TextRange::empty(0.into()), "test").with_related_many(vec![
        RelatedInfo::new(TextRange::empty(1.into()), "first"),
        RelatedInfo::new(TextRange::empty(2.into()), "second"),
    ]);
    assert_eq!(diag2.related.len(), 2);
}

#[test]
fn diagnostic_display() {
    let diag = Diagnostic::error(TextRange::new(5.into(), 10.into()), "test message");
    insta::assert_snapshot!(format!("{}", diag), @"error at 5..10: test message");

    let diag_with_fix = Diagnostic::error(TextRange::empty(0.into()), "msg")
        .with_fix(Fix::new("fix", "fix description"));
    insta::assert_snapshot!(format!("{}", diag_with_fix), @"error at 0..0: msg (fix: fix description)");

    let diag_with_related = Diagnostic::error(TextRange::empty(0.into()), "msg")
        .with_related(RelatedInfo::new(TextRange::new(1.into(), 2.into()), "rel"));
    insta::assert_snapshot!(format!("{}", diag_with_related), @"error at 0..0: msg (related: rel at 1..2)");
}

#[test]
fn render_options_constructors() {
    let plain = RenderOptions::plain();
    assert!(!plain.colored);

    let colored = RenderOptions::colored();
    assert!(colored.colored);

    let default = RenderOptions::default();
    assert!(default.colored);
}

#[test]
fn render_diagnostics_colored() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 5.into()), "test");
    let result = render_diagnostics("hello", &[diag], None, RenderOptions::colored());
    assert!(result.contains("test"));
    assert!(result.contains('\x1b'));
}

#[test]
fn render_diagnostics_empty() {
    let result = render_diagnostics("source", &[], None, RenderOptions::plain());
    assert!(result.is_empty());
}

#[test]
fn render_diagnostics_with_path() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 5.into()), "test error");
    let result = render_diagnostics(
        "hello world",
        &[diag],
        Some("test.pql"),
        RenderOptions::plain(),
    );
    insta::assert_snapshot!(result, @r"
    error: test error
     --> test.pql:1:1
      |
    1 | hello world
      | ^^^^^ test error
    ");
}

#[test]
fn render_diagnostics_zero_width_span() {
    let diag = Diagnostic::error(TextRange::empty(0.into()), "zero width error");
    let result = render_diagnostics("hello", &[diag], None, RenderOptions::plain());
    insta::assert_snapshot!(result, @r"
    error: zero width error
      |
    1 | hello
      | ^ zero width error
    ");
}

#[test]
fn render_diagnostics_with_related() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 5.into()), "primary").with_related(
        RelatedInfo::new(TextRange::new(6.into(), 10.into()), "related info"),
    );
    let result = render_diagnostics("hello world!", &[diag], None, RenderOptions::plain());
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
fn render_diagnostics_related_zero_width() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 5.into()), "primary").with_related(
        RelatedInfo::new(TextRange::empty(6.into()), "zero width related"),
    );
    let result = render_diagnostics("hello world!", &[diag], None, RenderOptions::plain());
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
fn render_diagnostics_with_fix() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 5.into()), "fixable")
        .with_fix(Fix::new("fixed", "apply this fix"));
    let result = render_diagnostics("hello world", &[diag], None, RenderOptions::plain());
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
fn render_diagnostics_multiple() {
    let diag1 = Diagnostic::error(TextRange::new(0.into(), 5.into()), "first error");
    let diag2 = Diagnostic::error(TextRange::new(6.into(), 10.into()), "second error");
    let result = render_diagnostics(
        "hello world!",
        &[diag1, diag2],
        None,
        RenderOptions::plain(),
    );
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
fn render_diagnostics_warning() {
    let diag = Diagnostic::warning(TextRange::new(0.into(), 5.into()), "a warning");
    let result = render_diagnostics("hello", &[diag], None, RenderOptions::plain());
    insta::assert_snapshot!(result, @r"
    warning: a warning
      |
    1 | hello
      | ^^^^^ a warning
    ");
}

#[test]
fn render_errors_wrapper() {
    let diag = Diagnostic::error(TextRange::new(0.into(), 3.into()), "test");
    let result = render_errors("abc", &[diag], Some("file.pql"));
    insta::assert_snapshot!(result, @r"
    error: test
     --> file.pql:1:1
      |
    1 | abc
      | ^^^ test
    ");
}
