use serde_json::json;

use super::wire::{InfoParts, info_json};

#[test]
fn session_info_uses_domain_names_at_the_protocol_boundary() {
    let info = info_json(InfoParts {
        module: None,
        query_tokens: json!([]),
        diagnostics: json!([]),
        typescript_declarations: String::new(),
        typescript_bindings: json!([]),
        entry_points: &[],
        bytecode_size_bytes: None,
    });

    assert_eq!(
        info,
        json!({
            "version": 1,
            "query_spans": [],
            "query_tokens": [],
            "diagnostics": [],
            "typescript_declarations": "",
            "typescript_bindings": [],
            "entry_points": [],
            "bytecode_size_bytes": null,
        })
    );
}
