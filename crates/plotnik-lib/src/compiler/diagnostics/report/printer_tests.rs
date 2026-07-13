//! Tests for diagnostic rendering (`DiagnosticsPrinter`) and the `Diagnostics`
//! collection that feeds it: report building, multi-span and multi-file layout,
//! suppression filtering (`live`), and the rendered text output.

use rowan::TextRange;

use crate::compiler::diagnostics::SourcePath;

use super::*;

#[test]
fn report_with_default_message() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedCaptureType,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn builder_with_related() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("primary")
        .related_to(
            Span::new(id, TextRange::new(6.into(), 10.into())),
            "related info",
        )
        .emit();

    assert_eq!(diagnostics.len(), 1);
    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: missing closing `)`: primary
      |
    1 | hello world!
      | ^^^^^ ---- related info
      |
    help: add `)` to close the node
    ");
}

#[test]
fn builder_with_all_options() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello world stuff!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("main error")
        .related_to(
            Span::new(id, TextRange::new(6.into(), 11.into())),
            "see also",
        )
        .related_to(
            Span::new(id, TextRange::new(12.into(), 17.into())),
            "and here",
        )
        .fix("try this", "HELLO")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: missing closing `)`: main error
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
    help: add `)` to close the node
    ");
}

#[test]
fn printer_colored() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::EmptyTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("test")
        .emit();

    let result = diagnostics.render_colored(&map, true);
    assert!(result.contains("test"));
    assert!(result.contains('\x1b'));
}

#[test]
fn printer_empty_diagnostics() {
    let map = SourceMap::from_inline("source");
    let diagnostics = Diagnostics::new();
    let result = diagnostics.render_raw(&map);
    assert!(result.is_empty());
}

#[test]
fn printer_with_custom_path() {
    let mut map = SourceMap::new();
    let id = map.add_file(SourcePath::new("test.pql"), "hello world");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("test error")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: `test error` is not defined
     --> test.pql:1:1
      |
    1 | hello world
      | ^^^^^
      |
    help: `(Name)` uses a definition; define `Name = ...` or check the spelling
    ");
}

#[test]
fn printer_empty_range() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedExpression,
            Span::new(id, TextRange::empty(0.into())),
        )
        .detail("empty range error")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @r#"
    error: expected an expression: empty range error
      |
    1 | hello
      | ^
      |
    help: an expression is a node `(kind)`, anonymous node `"text"`, sequence `{...}`, or alternation `[...]`
    "#);
}

#[test]
fn printer_related_empty_range() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("primary")
        .related_to(
            Span::new(id, TextRange::empty(6.into())),
            "empty range related",
        )
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: missing closing `)`: primary
      |
    1 | hello world!
      | ^^^^^ - empty range related
      |
    help: add `)` to close the node
    ");
}

#[test]
fn printer_multiple_diagnostics() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello world!");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("first error")
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(id, TextRange::new(6.into(), 10.into())),
        )
        .detail("second error")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: missing closing `)`: first error
      |
    1 | hello world!
      | ^^^^^
      |
    help: add `)` to close the node

    error: `second error` is not defined
      |
    1 | hello world!
      |       ^^^^
      |
    help: `(Name)` uses a definition; define `Name = ...` or check the spelling
    ");
}

#[test]
fn diagnostics_collection_methods() {
    let mut map = SourceMap::new();
    let id = map.add_inline("ab");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::empty(0.into())),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(id, TextRange::empty(1.into())),
        )
        .emit();

    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.has_errors());
    assert_eq!(diagnostics.error_count(), 2);
}

#[test]
fn filtered_no_suppression_disjoint_spans() {
    let mut map = SourceMap::new();
    let id = map.add_inline("0123456789012345");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(id, TextRange::new(10.into(), 15.into())),
        )
        .emit();

    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filtered_suppresses_lower_priority_contained() {
    let mut map = SourceMap::new();
    let id = map.add_inline("01234567890123456789");

    let mut diagnostics = Diagnostics::new();
    // Higher priority error (UnclosedTree) contains lower priority (MissingDefName)
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 20.into())),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::MissingDefName,
            Span::new(id, TextRange::new(5.into(), 15.into())),
        )
        .emit();

    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_consequence_suppressed_by_structural() {
    let mut map = SourceMap::new();
    let id = map.add_inline("01234567890123456789");

    let mut diagnostics = Diagnostics::new();
    // Consequence error (MissingDefName) suppressed when structural error (UnclosedTree) exists
    diagnostics
        .report(
            DiagnosticKind::MissingDefName,
            Span::new(id, TextRange::new(0.into(), 20.into())),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(5.into(), 15.into())),
        )
        .emit();

    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_same_span_higher_priority_wins() {
    let mut map = SourceMap::new();
    let id = map.add_inline("0123456789");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 10.into())),
        )
        .emit();
    diagnostics
        .report(
            DiagnosticKind::MissingDefName,
            Span::new(id, TextRange::new(0.into(), 10.into())),
        )
        .emit();

    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].kind, DiagnosticKind::UnclosedTree);
}

#[test]
fn filtered_no_cross_file_containment_suppression() {
    let mut map = SourceMap::new();
    let file_a = map.add_file(SourcePath::new("a.ptk"), "01234567890123456789");
    let file_b = map.add_file(SourcePath::new("b.ptk"), "0123456789");

    let mut diagnostics = Diagnostics::new();
    // File A: a structural error whose suppression range spans the whole file.
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(file_a, TextRange::new(0.into(), 5.into())),
        )
        .suppression_range(TextRange::new(0.into(), 20.into()))
        .emit();
    // File B: a real error at an offset that numerically falls inside A's range.
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(file_b, TextRange::new(5.into(), 10.into())),
        )
        .emit();

    // Without the source guard, A would swallow B via Rule 1 (contains + suppresses).
    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 2);
}

#[test]
fn filtered_no_cross_file_consequence_suppression() {
    let mut map = SourceMap::new();
    let file_a = map.add_file(SourcePath::new("a.ptk"), "01234567890123456789");
    let file_b = map.add_file(SourcePath::new("b.ptk"), "0123456789");

    let mut diagnostics = Diagnostics::new();
    // File A: a consequence error, with no root diagnostic in its own source.
    diagnostics
        .report(
            DiagnosticKind::MissingDefName,
            Span::new(file_a, TextRange::new(0.into(), 5.into())),
        )
        .emit();
    // File B: a root diagnostic, in a different source.
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(file_b, TextRange::new(0.into(), 5.into())),
        )
        .emit();

    // Rule 3 is per-source: B's root error must not suppress A's consequence.
    let filtered = diagnostics.live();
    assert_eq!(filtered.len(), 2);
    assert!(
        filtered
            .iter()
            .any(|m| m.kind == DiagnosticKind::MissingDefName)
    );
}

#[test]
fn filtered_empty_diagnostics() {
    let diagnostics = Diagnostics::new();
    let filtered = diagnostics.live();
    assert!(filtered.is_empty());
}

#[test]
fn render_filtered() {
    let mut map = SourceMap::new();
    let id = map.add_inline("(function_declaration");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 20.into())),
        )
        .detail("unclosed tree")
        .emit();
    diagnostics
        .report(
            DiagnosticKind::MissingDefName,
            Span::new(id, TextRange::new(5.into(), 15.into())),
        )
        .detail("unnamed def")
        .emit();

    let result = diagnostics.render(&map);
    assert!(result.contains("unclosed tree"));
    assert!(!result.contains("unnamed def"));
}

#[test]
fn multi_file_cross_file_related() {
    let mut map = SourceMap::new();
    let file_a = map.add_file(SourcePath::new("a.ptk"), "Foo = (bar)");
    let file_b = map.add_file(SourcePath::new("b.ptk"), "(Foo) @x");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UndefinedReference,
            Span::new(file_b, TextRange::new(1.into(), 4.into())),
        )
        .detail("Foo")
        .related_to(
            Span::new(file_a, TextRange::new(0.into(), 3.into())),
            "defined here",
        )
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
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
      |
    help: `(Name)` uses a definition; define `Name = ...` or check the spelling
    ");
}

#[test]
fn multi_file_same_file_related() {
    let mut map = SourceMap::new();
    let file_a = map.add_file(SourcePath::new("main.ptk"), "Foo = (bar) Foo = (baz)");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::DuplicateDefinition,
            Span::new(file_a, TextRange::new(12.into(), 15.into())),
        )
        .detail("Foo")
        .related_to(
            Span::new(file_a, TextRange::new(0.into(), 3.into())),
            "first defined here",
        )
        .emit();

    let result = diagnostics.render_raw(&map);
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
