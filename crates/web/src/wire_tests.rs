use serde_json::json;

use plotnik_compiler::QueryToken;
use plotnik_compiler::bytecode::{Labeling, SpanEntry, SpanKind};

use super::wire::{InfoParts, info_json, query_span_json};

#[test]
fn session_info_uses_domain_names_at_the_protocol_boundary() {
    let info = info_json(InfoParts {
        module: None,
        query_tokens: json!([QueryToken {
            kind: "ident",
            span: (0, 1),
        }]),
        diagnostics: json!([]),
        entrypoints: &[],
        bytecode_size_bytes: None,
    });

    assert_eq!(
        info,
        json!({
            "version": 0,
            "query_spans": [],
            "query_tokens": [{ "kind": "ident", "span": [0, 1] }],
            "diagnostics": [],
            "entrypoints": [],
            "bytecode_size_bytes": null,
        })
    );
}

#[test]
fn query_span_qualifies_ids_and_separates_alternation_labeling() {
    let span = query_span_json(
        3,
        SpanEntry {
            source_id: 2,
            kind: SpanKind::Alternation(Labeling::Labeled),
            start: 5,
            end: 12,
            type_id: 4,
            member: 7,
        },
    );

    assert_eq!(
        span,
        json!({
            "id": 3,
            "source_id": 2,
            "kind": "alternation",
            "labeling": "labeled",
            "span": [5, 12],
            "binding": {
                "type_id": 4,
                "member_id": 7,
            },
        })
    );
}
