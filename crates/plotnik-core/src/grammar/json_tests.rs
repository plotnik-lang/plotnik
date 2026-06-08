use super::Grammar;
use super::raw::{RawGrammar, RawRule};

#[test]
fn parse_minimal_grammar() {
    let json = r#"{
        "name": "test",
        "rules": {
            "source_file": { "type": "SYMBOL", "name": "expression" },
            "expression": { "type": "STRING", "value": "x" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();

    assert_eq!(raw.name, "test");
    assert_eq!(grammar.name(), "test");
    assert_eq!(raw.rules.len(), 2);
}

#[test]
fn parse_seq_and_choice() {
    let json = r#"{
        "name": "test",
        "rules": {
            "root": {
                "type": "SEQ",
                "members": [
                    { "type": "STRING", "value": "a" },
                    { "type": "CHOICE", "members": [
                        { "type": "STRING", "value": "b" },
                        { "type": "BLANK" }
                    ]}
                ]
            }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();

    assert!(matches!(raw.rules["root"], RawRule::SEQ { .. }));
}

#[test]
fn parse_field() {
    let json = r#"{
        "name": "test",
        "rules": {
            "func": {
                "type": "FIELD",
                "name": "name",
                "content": { "type": "SYMBOL", "name": "identifier" }
            },
            "identifier": { "type": "PATTERN", "value": "[a-z]+" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();

    assert!(matches!(raw.rules["func"], RawRule::FIELD { .. }));
}

#[test]
fn preserves_rule_order() {
    let json = r#"{
        "name": "test",
        "rules": {
            "program": { "type": "SYMBOL", "name": "statement" },
            "statement": { "type": "SYMBOL", "name": "expression" },
            "expression": { "type": "STRING", "value": "x" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let rule_names = raw.rules.keys().map(String::as_str).collect::<Vec<_>>();

    assert_eq!(rule_names, ["program", "statement", "expression"]);
}

#[test]
fn postcard_round_trips_raw_grammar() {
    let json = r#"{
        "name": "test",
        "rules": {
            "program": { "type": "STRING", "value": "x" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let encoded_json = raw.to_json().unwrap();
    assert_eq!(RawGrammar::from_json(&encoded_json).unwrap(), raw);

    let bytes = raw.to_postcard().unwrap();
    let decoded = RawGrammar::from_postcard(&bytes).unwrap();

    assert_eq!(decoded, raw);
}
