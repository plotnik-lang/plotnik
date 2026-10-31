use super::*;

fn raw(rules: serde_json::Value) -> RawGrammar {
    let mut raw: RawGrammar =
        serde_json::from_value(serde_json::json!({"name": "groups", "rules": rules})).unwrap();
    let program = raw.rules.shift_remove("program").unwrap();
    raw.rules.shift_insert(0, "program".to_owned(), program);
    raw
}

#[test]
fn invalid_group_names_and_collisions_are_rejected() {
    for name in [
        "value__",
        "value__group__nested",
        "Value__leaf",
        "value___leaf",
    ] {
        let grammar = raw(serde_json::json!({
            "program": {"type": "SYMBOL", "name": name},
            (name): {"type": "STRING", "value": "unit"}
        }));

        assert!(Grammar::from_raw(&grammar).is_err(), "{name}");
    }

    let grammar = raw(serde_json::json!({
        "program": {"type": "CHOICE", "members": [
            {"type": "SYMBOL", "name": "value__leaf"},
            {"type": "SYMBOL", "name": "value__leaf\0other"},
            {"type": "SYMBOL", "name": "value__group"}
        ]},
        "value__leaf": {"type": "STRING", "value": "a"},
        "value__leaf\0other": {"type": "STRING", "value": "b"},
        "value__group": {"type": "STRING", "value": "c"}
    }));

    assert!(Grammar::from_raw(&grammar).is_err());

    let grammar = raw(serde_json::json!({
        "program": {"type": "CHOICE", "members": [
            {"type": "SYMBOL", "name": "value"},
            {"type": "SYMBOL", "name": "value__leaf"}
        ]},
        "value": {"type": "STRING", "value": "other"},
        "value__leaf": {"type": "STRING", "value": "unit"}
    }));

    assert!(Grammar::from_raw(&grammar).is_err());
}

#[test]
fn aliases_cannot_change_group_identity() {
    for (name, alias) in [("value__leaf", "other"), ("other", "value__leaf")] {
        let grammar = raw(serde_json::json!({
            "program": {"type": "ALIAS", "named": true, "value": alias,
                "content": {"type": "SYMBOL", "name": name}},
            (name): {"type": "STRING", "value": "unit"}
        }));

        assert!(Grammar::from_raw(&grammar).is_err());
    }
}
