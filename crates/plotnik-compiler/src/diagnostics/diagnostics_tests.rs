use rowan::TextRange;

use super::*;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn report_with_default_message() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::ExpectedTypeName,
            TextRange::new(0.into(), 5.into()),
        )
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn report_with_custom_message() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::ExpectedTypeName,
            TextRange::new(0.into(), 5.into()),
        )
        .message("expected type name after '::' (e.g., ::MyType)")
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn builder_with_related() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("primary")
        .related_to(id, TextRange::new(6.into(), 10.into()), "related info")
        .emit();

    assert_eq!(diagnostics.len(), 1);
    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; primary
      |
    1 | hello world!
      | ^^^^^ ---- related info
    ");
}

#[test]
fn builder_with_fix() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello world");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::InvalidFieldEquals,
            TextRange::new(0.into(), 5.into()),
        )
        .message("fixable")
        .fix("apply this fix", "fixed")
        .emit();

    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: use `:` instead of `=`: fixable
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello world stuff!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("main error")
        .related_to(id, TextRange::new(6.into(), 11.into()), "see also")
        .related_to(id, TextRange::new(12.into(), 17.into()), "and here")
        .fix("try this", "HELLO")
        .emit();

    let result = diagnostics.printer(&map).render();
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::EmptyTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("test")
        .emit();

    let result = diagnostics.printer(&map).colored(true).render();
    assert!(result.contains("test"));
    assert!(result.contains('\x1b'));
}

#[test]
fn printer_empty_diagnostics() {
    let map = SourceMap::one_liner("source");
    let diagnostics = Diagnostics::new();
    let result = diagnostics.printer(&map).render();
    assert!(result.is_empty());
}

#[test]
fn printer_with_custom_path() {
    let mut map = SourceMap::new();
    let id = map.add_file("test.pql", "hello world");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::UndefinedReference,
            TextRange::new(0.into(), 5.into()),
        )
        .message("test error")
        .emit();

    let result = diagnostics.printer(&map).render();
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::ExpectedExpression,
            TextRange::empty(0.into()),
        )
        .message("zero width error")
        .emit();

    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: expected an expression: zero width error
      |
    1 | hello
      | ^
    ");
}

#[test]
fn printer_related_zero_width() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("primary")
        .related_to(id, TextRange::empty(6.into()), "zero width related")
        .emit();

    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: missing closing `)`; primary
      |
    1 | hello world!
      | ^^^^^ - zero width related
    ");
}

#[test]
fn printer_multiple_diagnostics() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .message("first error")
        .emit();
    diagnostics
        .report(
            id,
            DiagnosticKind::UndefinedReference,
            TextRange::new(6.into(), 10.into()),
        )
        .message("second error")
        .emit();

    let result = diagnostics.printer(&map).render();
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("ab");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(id, DiagnosticKind::UnclosedTree, TextRange::empty(0.into()))
        .emit();
    diagnostics
        .report(
            id,
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
        DiagnosticKind::UnnamedDef.default_severity(),
        Severity::Error
    );
}

#[test]
fn diagnostic_kind_suppression_order() {
    // Higher priority (earlier in enum) suppresses lower priority (later in enum)
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnnamedDef));
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UndefinedReference));
    assert!(DiagnosticKind::ExpectedExpression.suppresses(&DiagnosticKind::UnnamedDef));

    // Same kind doesn't suppress itself
    assert!(!DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnclosedTree));

    // Lower priority doesn't suppress higher priority
    assert!(!DiagnosticKind::UnnamedDef.suppresses(&DiagnosticKind::UnclosedTree));
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

#[test]
fn filtered_no_suppression_disjoint_spans() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("0123456789012345");

    let mut diagnostics = Diagnostics::new();
    // Two errors at different positions - both should show
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 5.into()),
        )
        .emit();
    diagnostics
        .report(
            id,
            DiagnosticKind::UndefinedReference,
            TextRange::new(10.into(), 15.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filtered_suppresses_lower_priority_contained() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("01234567890123456789");

    let mut diagnostics = Diagnostics::new();
    // Higher priority error (UnclosedTree) contains lower priority (UnnamedDef)
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 20.into()),
        )
        .emit();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnnamedDef,
            TextRange::new(5.into(), 15.into()),
        )
        .emit();

    let filtered = diagnostics.filtered();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_consequence_suppressed_by_structural() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("01234567890123456789");

    let mut diagnostics = Diagnostics::new();
    // Consequence error (UnnamedDef) suppressed when structural error (UnclosedTree) exists
    diagnostics
        .report(
            id,
            DiagnosticKind::UnnamedDef,
            TextRange::new(0.into(), 20.into()),
        )
        .emit();
    diagnostics
        .report(
            id,
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("0123456789");

    let mut diagnostics = Diagnostics::new();
    // Two errors at exact same span
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 10.into()),
        )
        .emit();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnnamedDef,
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
    let mut map = SourceMap::new();
    let id = map.add_one_liner("(function_declaration");

    let mut diagnostics = Diagnostics::new();
    // Add overlapping errors where one should be suppressed
    diagnostics
        .report(
            id,
            DiagnosticKind::UnclosedTree,
            TextRange::new(0.into(), 20.into()),
        )
        .message("unclosed tree")
        .emit();
    diagnostics
        .report(
            id,
            DiagnosticKind::UnnamedDef,
            TextRange::new(5.into(), 15.into()),
        )
        .message("unnamed def")
        .emit();

    let result = diagnostics.render_filtered(&map);
    // Should only show the unclosed tree error
    assert!(result.contains("unclosed tree"));
    assert!(!result.contains("unnamed def"));
}

// Multi-file diagnostics tests

#[test]
fn multi_file_cross_file_related() {
    let mut map = SourceMap::new();
    let file_a = map.add_file("a.ptk", "Foo = (bar)");
    let file_b = map.add_file("b.ptk", "(Foo) @x");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            file_b,
            DiagnosticKind::UndefinedReference,
            TextRange::new(1.into(), 4.into()),
        )
        .message("Foo")
        .related_to(file_a, TextRange::new(0.into(), 3.into()), "defined here")
        .emit();

    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: `Foo` is not defined
     --> b.ptk:1:2
      |
    1 | (Foo) @x
      |  ^^^
      |
     ::: a.ptk:1:1
      |
    1 | Foo = (bar)
      | --- defined here
    ");
}

#[test]
fn multi_file_same_file_related() {
    let mut map = SourceMap::new();
    let file_a = map.add_file("main.ptk", "Foo = (bar) Foo = (baz)");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            file_a,
            DiagnosticKind::DuplicateDefinition,
            TextRange::new(12.into(), 15.into()),
        )
        .message("Foo")
        .related_to(
            file_a,
            TextRange::new(0.into(), 3.into()),
            "first defined here",
        )
        .emit();

    let result = diagnostics.printer(&map).render();
    insta::assert_snapshot!(result, @r"
    error: `Foo` is already defined
     --> main.ptk:1:13
      |
    1 | Foo = (bar) Foo = (baz)
      | ---         ^^^
      | |
      | first defined here
    ");
}

#[test]
fn source_map_iteration() {
    let mut map = SourceMap::new();
    map.add_file("a.ptk", "content a");
    map.add_file("b.ptk", "content b");

    assert_eq!(map.len(), 2);
    assert!(!map.is_empty());

    let contents: Vec<_> = map.iter().map(|s| s.content).collect();
    assert_eq!(contents, vec!["content a", "content b"]);
}

#[test]
fn span_new() {
    let mut map = SourceMap::new();
    let id = map.add_one_liner("hello");

    let range = TextRange::new(5.into(), 10.into());
    let span = Span::new(id, range);
    assert_eq!(span.source, id);
    assert_eq!(span.range, range);
}
