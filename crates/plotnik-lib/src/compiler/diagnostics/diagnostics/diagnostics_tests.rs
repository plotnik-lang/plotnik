use rowan::TextRange;

use crate::compiler::diagnostics::SourcePath;

use super::*;

#[test]
fn severity_display() {
    insta::assert_snapshot!(format!("{}", Severity::Error), @"error");
    insta::assert_snapshot!(format!("{}", Severity::Warning), @"warning");
}

#[test]
fn report_with_default_message() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedTypeName,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .emit();

    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics.has_errors());
}

#[test]
fn report_with_custom_message() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedTypeName,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("expected type name after '::' (e.g., ::MyType)")
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
fn builder_with_fix() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello world");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::InvalidFieldEquals,
            Span::new(id, TextRange::new(0.into(), 5.into())),
        )
        .detail("fixable")
        .fix("apply this fix", "fixed")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: fields use `:`, not `=`: fixable
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
fn printer_zero_width_span() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::ExpectedExpression,
            Span::new(id, TextRange::empty(0.into())),
        )
        .detail("zero width error")
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @r#"
    error: expected an expression: zero width error
      |
    1 | hello
      | ^
      |
    help: an expression is a node `(kind)`, anonymous node `"text"`, sequence `{...}`, or alternation `[...]`
    "#);
}

#[test]
fn printer_related_zero_width() {
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
            "zero width related",
        )
        .emit();

    let result = diagnostics.render_raw(&map);
    insta::assert_snapshot!(result, @"
    error: missing closing `)`: primary
      |
    1 | hello world!
      | ^^^^^ - zero width related
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
fn diagnostic_kind_default_severity() {
    assert_eq!(DiagnosticKind::UnclosedTree.severity(), Severity::Error);
    assert_eq!(DiagnosticKind::MissingDefName.severity(), Severity::Error);
}

#[test]
fn diagnostic_kind_suppression_order() {
    // Higher priority (earlier in enum) suppresses lower priority (later in enum)
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::MissingDefName));
    assert!(DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UndefinedReference));
    assert!(DiagnosticKind::ExpectedExpression.suppresses(&DiagnosticKind::MissingDefName));

    // Same kind doesn't suppress itself
    assert!(!DiagnosticKind::UnclosedTree.suppresses(&DiagnosticKind::UnclosedTree));

    // Lower priority doesn't suppress higher priority
    assert!(!DiagnosticKind::MissingDefName.suppresses(&DiagnosticKind::UnclosedTree));
}

#[test]
fn diagnostic_kind_fallback_messages() {
    assert_eq!(
        DiagnosticKind::UnclosedTree.summary(),
        "missing closing `)`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedSequence.summary(),
        "missing closing `}`"
    );
    assert_eq!(
        DiagnosticKind::UnclosedAlternation.summary(),
        "missing closing `]`"
    );
    assert_eq!(
        DiagnosticKind::ExpectedExpression.summary(),
        "expected an expression"
    );
}

#[test]
fn diagnostic_kind_custom_messages() {
    // Detail replaces the whole message
    assert_eq!(DiagnosticKind::UnexpectedToken.template(), "{}");
    // Detail is interpolated into a template
    assert_eq!(
        DiagnosticKind::UndefinedReference.template(),
        "`{}` is not defined"
    );
    // Default: fallback message plus detail
    assert_eq!(
        DiagnosticKind::DuplicateCaptureInScope.template(),
        "capture `@{}` already defined in this scope"
    );
}

#[test]
fn diagnostic_kind_message_rendering() {
    // No custom message → fallback
    assert_eq!(
        DiagnosticKind::UnclosedTree.render(None),
        "missing closing `)`"
    );
    // With custom message → template applied
    assert_eq!(
        DiagnosticKind::UnexpectedToken.render(Some("expected `)`")),
        "expected `)`"
    );
    assert_eq!(
        DiagnosticKind::UndefinedReference.render(Some("Foo")),
        "`Foo` is not defined"
    );
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

#[test]
fn source_map_iteration() {
    let mut map = SourceMap::new();
    map.add_file(SourcePath::new("a.ptk"), "content a");
    map.add_file(SourcePath::new("b.ptk"), "content b");

    assert_eq!(map.len(), 2);
    assert!(!map.is_empty());

    let contents: Vec<_> = map.iter().map(|s| s.content).collect();
    assert_eq!(contents, vec!["content a", "content b"]);
}

#[test]
fn span_new() {
    let mut map = SourceMap::new();
    let id = map.add_inline("hello");

    let range = TextRange::new(5.into(), 10.into());
    let span = Span::new(id, range);
    assert_eq!(span.source, id);
    assert_eq!(span.range, range);
}

#[test]
fn render_json_full_shape() {
    let mut map = SourceMap::new();
    let id = map.add_file(SourcePath::new("query.ptk"), "(foo)\n(bar)");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnknownNodeKind,
            Span::new(id, TextRange::new(7.into(), 10.into())),
        )
        .detail("bar")
        .related_to(
            Span::new(id, TextRange::new(1.into(), 4.into())),
            "first seen here",
        )
        .fix("replace with `baz`", "baz")
        .hint("check the grammar")
        .emit();

    let json: serde_json::Value = serde_json::from_str(&diagnostics.render_json(&map)).unwrap();

    insta::assert_snapshot!(serde_json::to_string_pretty(&json).unwrap(), @r#"
    [
      {
        "code": "unknown_node_kind",
        "fix": {
          "description": "replace with `baz`",
          "replacement": "baz"
        },
        "hints": [
          "check the grammar"
        ],
        "message": "`bar` is not a valid node kind",
        "related": [
          {
            "message": "first seen here",
            "span": {
              "end": {
                "column": 5,
                "line": 1,
                "offset": 4
              },
              "file": "query.ptk",
              "start": {
                "column": 2,
                "line": 1,
                "offset": 1
              }
            }
          }
        ],
        "severity": "error",
        "span": {
          "end": {
            "column": 5,
            "line": 2,
            "offset": 10
          },
          "file": "query.ptk",
          "start": {
            "column": 2,
            "line": 2,
            "offset": 7
          }
        }
      }
    ]
    "#);
}

#[test]
fn render_json_minimal_omits_empty_fields() {
    let mut map = SourceMap::new();
    let id = map.add_inline("(foo");

    let mut diagnostics = Diagnostics::new();
    diagnostics
        .report(
            DiagnosticKind::UnclosedTree,
            Span::new(id, TextRange::new(0.into(), 4.into())),
        )
        .emit();

    let json: serde_json::Value = serde_json::from_str(&diagnostics.render_json(&map)).unwrap();

    insta::assert_snapshot!(serde_json::to_string_pretty(&json).unwrap(), @r#"
    [
      {
        "code": "unclosed_tree",
        "hints": [
          "add `)` to close the node"
        ],
        "message": "missing closing `)`",
        "severity": "error",
        "span": {
          "end": {
            "column": 5,
            "line": 1,
            "offset": 4
          },
          "file": "<query>",
          "start": {
            "column": 1,
            "line": 1,
            "offset": 0
          }
        }
      }
    ]
    "#);
}
