use rowan::TextRange;

use super::*;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn error_builder() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("test error", TextRange::new(0.into(), 5.into()))
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
    assert!(!diagnostics.has_warnings());
}

#[test]
fn warning_builder() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .warning("test warning", TextRange::new(0.into(), 5.into()))
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(!diagnostics.has_errors());
    assert!(diagnostics.has_warnings());
}

#[test]
fn builder_with_related() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("primary", TextRange::new(0.into(), 5.into()))
        .related_to("related info", TextRange::new(6.into(), 10.into()))
        .emit();

    assert_eq!(diagnostics.len(), 1);
    let result = diagnostics.printer("hello world!").render();
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
fn builder_with_fix() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("fixable", TextRange::new(0.into(), 5.into()))
        .fix("apply this fix", "fixed")
        .emit();

    let result = diagnostics.printer("hello world").render();
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
fn builder_with_all_options() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("main error", TextRange::new(0.into(), 5.into()))
        .related_to("see also", TextRange::new(6.into(), 11.into()))
        .related_to("and here", TextRange::new(12.into(), 17.into()))
        .fix("try this", "HELLO")
        .emit();

    let result = diagnostics.printer("hello world stuff!").render();
    insta::assert_snapshot!(result, @r"
    error: main error
      |
    1 | hello world stuff!
      | ^^^^^ ----- ----- and here
      | |     |
      | |     see also
      | main error
      |
    help: try this
      |
    1 - hello world stuff!
    1 + HELLO world stuff!
      |
    ");
}

#[test]
fn printer_colored() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("test", TextRange::new(0.into(), 5.into()))
        .emit();

    let result = diagnostics.printer("hello").colored(true).render();
    assert!(result.contains("test"));
    assert!(result.contains('\x1b'));
}

#[test]
fn printer_empty_diagnostics() {
    let diagnostics = Diagnostics::new();
    let result = diagnostics.printer("source").render();
    assert!(result.is_empty());
}

#[test]
fn printer_with_path() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("test error", TextRange::new(0.into(), 5.into()))
        .emit();

    let result = diagnostics.printer("hello world").path("test.pql").render();
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
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("zero width error", TextRange::empty(0.into()))
        .emit();

    let result = diagnostics.printer("hello").render();
    insta::assert_snapshot!(result, @r"
    error: zero width error
      |
    1 | hello
      | ^ zero width error
    ");
}

#[test]
fn printer_related_zero_width() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("primary", TextRange::new(0.into(), 5.into()))
        .related_to("zero width related", TextRange::empty(6.into()))
        .emit();

    let result = diagnostics.printer("hello world!").render();
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
fn printer_multiple_diagnostics() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("first error", TextRange::new(0.into(), 5.into()))
        .emit();
    diagnostics
        .error("second error", TextRange::new(6.into(), 10.into()))
        .emit();

    let result = diagnostics.printer("hello world!").render();
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
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .warning("a warning", TextRange::new(0.into(), 5.into()))
        .emit();

    let result = diagnostics.printer("hello").render();
    insta::assert_snapshot!(result, @r"
    warning: a warning
      |
    1 | hello
      | ^^^^^ a warning
    ");
}

#[test]
fn diagnostics_collection_methods() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("error", TextRange::empty(0.into()))
        .emit();
    diagnostics
        .warning("warning", TextRange::empty(1.into()))
        .emit();

    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.has_errors());
    assert!(diagnostics.has_warnings());
    assert_eq!(diagnostics.error_count(), 1);
    assert_eq!(diagnostics.warning_count(), 1);
}
