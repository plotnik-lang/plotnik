//! Tests for `Diagnostics::render_json`: the machine-readable wire shape,
//! byte→line/column mapping, and omission of empty fields.

use rowan::TextRange;

use crate::compiler::diagnostics::SourcePath;

use super::*;

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
