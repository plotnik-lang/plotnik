use rowan::TextRange;

use super::*;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn report_with_default_message() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedTypeName,
            TextRange::new(0.into(), 5.into()),
        )
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn report_with_custom_message() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedTypeName,
            TextRange::new(0.into(), 5.into()),
        )
        .message("expected type name after '::' (e.g., ::MyType)")
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn error_builder_legacy() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .error("test error", TextRange::new(0.into(), 5.into()))
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn builder_with_related() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("primary")
        .related_to("related info", TextRange::new(6.into(), 10.into()))
        .emit();

    assert_eq!(diagnostics.len(), 1);
    let result = diagnostics.printer("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; primary
      |
    1 | hello world!
      | ^^^^^ ---- related info
    ");
}

#[test]
fn builder_with_fix() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::InvalidFieldEquals,
            TextRange::new(0.into(), 5.into()),
        )
        .message("fixable")
        .fix("apply this fix", "fixed")
        .emit();

    let result = diagnostics.printer("hello world").render();
    insta::assert_snapshot!(result, @r"
    error: use `:` for field constraints, not `=`; fixable
      |
    1 | hello world
      | ^^^^^
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
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("main error")
        .related_to("see also", TextRange::new(6.into(), 11.into()))
        .related_to("and here", TextRange::new(12.into(), 17.into()))
        .fix("try this", "HELLO")
        .emit();

    let result = diagnostics.printer("hello world stuff!").render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; main error
      |
    1 | hello world stuff!
      | ^^^^^ ----- ----- and here
      |       |
      |       see also
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
        .report(
            DiagnosticKind::EmptyTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("test")
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
        .report(
            DiagnosticKind::UndefinedReference,
            TextRange::new(0.into(), 5.into()),
        )
        .message("test error")
        .emit();

    let result = diagnostics.printer("hello world").path("test.pql").render();
    insta::assert_snapshot!(result, @r"
    error: `test error` is not defined
     --> test.pql:1:1
      |
    1 | hello world
      | ^^^^^
    ");
}

#[test]
fn printer_zero_width_span() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedExpression,
            TextRange::empty(0.into()),
        )
        .message("zero width error")
        .emit();

    let result = diagnostics.printer("hello").render();
    insta::assert_snapshot!(result, @r"
    error: expected an expression; zero width error
      |
    1 | hello
      | ^
    ");
}

#[test]
fn printer_related_zero_width() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("primary")
        .related_to("zero width related", TextRange::empty(6.into()))
        .emit();

    let result = diagnostics.printer("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; primary
      |
    1 | hello world!
      | ^^^^^ - zero width related
    ");
}

#[test]
fn printer_multiple_diagnostics() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("first error")
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            TextRange::new(6.into(), 10.into()),
        )
        .message("second error")
        .emit();

    let result = diagnostics.printer("hello world!").render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; first error
      |
    1 | hello world!
      | ^^^^^

    error: `second error` is not defined
      |
    1 | hello world!
      |       ^^^^
    ");
}

#[test]
fn diagnostics_collection_methods() {
    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(DiagnosticKind::UnclosedTree, TextRange::empty(0.into()))
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            TextRange::empty(1.into()),
        )
        .emit();

    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.has_errors());
    assert_eq!(diagnostics.error_count(), 2);
}

#[test]
fn diagnostic_kind_default_severity() {
    assert_eq!(
        DiagnosticKind::UnclosedTree.default_severity(),
        Severity::Error
    );
    assert_eq!(
        DiagnosticKind::UnnamedDefNotLast.default_severity(),
        Severity::Error
    );
}

#[test]
fn diagnostic_kind_suppression_order() {
    // Higher priority (earlier in enum) suppresses lower priority (later in enum)
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnnamedDefNotLast));
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UndefinedReference));
    assert!(DiagnosticKind::ExpectedExpression.suppresses(&DiagnosticKind::UnnamedDefNotLast));

    // Same kind doesn't suppress itself
    assert!(!DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnclosedTree));

    // Lower priority doesn't suppress higher priority
    assert!(!DiagnosticKind::UnnamedDefNotLast.suppresses(&DiagnosticKind::UnclosedTree));
}

#[test]
fn diagnostic_kind_fallback_messages() {
    assert_eq!(
        DiagnosticKind::UnclosedTree.fallback_message(),
        "missing closing `)`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedSequence.fallback_message(),
        "missing closing `}`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedAlternation.fallback_message(),
        "missing closing `]`"
    );
    assert_eq!(
        DiagnosticKind::ExpectedExpression.fallback_message(),
        "expected an expression"
    );
}

#[test]
fn diagnostic_kind_custom_messages() {
    assert_eq!(
        DiagnosticKind::UnclosedTree.custom_message(),
        "missing closing `)`; {}"
    );
    assert_eq!(
        DiagnosticKind::UndefinedReference.custom_message(),
        "`{}` is not defined"
    );
}

#[test]
fn diagnostic_kind_message_rendering() {
    // No custom message → fallback
    assert_eq!(
        DiagnosticKind::UnclosedTree.message(None),
        "missing closing `)`"
    );
    // With custom message → template applied
    assert_eq!(
        DiagnosticKind::UnclosedTree.message(Some("expected `)`")),
        "missing closing `)`; expected `)`"
    );
    assert_eq!(
        DiagnosticKind::UndefinedReference.message(Some("Foo")),
        "`Foo` is not defined"
    );
}

// === Filtering/suppression tests ===

#[test]
fn filtered_no_suppression_disjoint_spans() {
    let mut diagnostics = Diagnostics::new();
    // Two errors at different positions - both should show
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            TextRange::new(10.into(), 15.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filtered_suppresses_lower_priority_contained() {
    let mut diagnostics = Diagnostics::new();
    // Higher priority error (UnclosedTree) contains lower priority (UnnamedDefNotLast)
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 20.into()),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UnnamedDefNotLast,
            TextRange::new(5.into(), 15.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_consequence_suppressed_by_structural() {
    let mut diagnostics = Diagnostics::new();
    // Consequence error (UnnamedDefNotLast) suppressed when structural error (UnclosedTree) exists
    diagnostics
        .report(
            DiagnosticKind::UnnamedDefNotLast,
            TextRange::new(0.into(), 20.into()),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(5.into(), 15.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    // Only UnclosedTree remains - consequence errors suppressed when primary errors exist
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_same_span_higher_priority_wins() {
    let mut diagnostics = Diagnostics::new();
    // Two errors at exact same span
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 10.into()),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UnnamedDefNotLast,
            TextRange::new(0.into(), 10.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_empty_diagnostics() {
    let diagnostics = Diagnostics::new();
    let filtered = diagnostics.filtered();
    assert!(filtered.is_empty());
}

#[test]
fn render_filtered() {
    let mut diagnostics = Diagnostics::new();
    // Add overlapping errors where one should be suppressed
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 20.into()),
        )
        .message("unclosed tree")
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UnnamedDefNotLast,
            TextRange::new(5.into(), 15.into()),
        )
        .message("unnamed def")
        .emit();

    let result = diagnostics.render_filtered("(function_declaration");
    // Should only show the unclosed tree error
    assert!(result.contains("unclosed tree"));
    assert!(!result.contains("unnamed def"));
}
