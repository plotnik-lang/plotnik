use super::Grammar;
use super::raw::RawGrammar;

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
fn exposes_nul_anonymous_node_as_empty_string() {
    let json = r#"{
        "name": "test",
        "rules": {
            "source_file": { "type": "STRING", "value": "\u0000" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();

    assert!(grammar.resolve_anonymous_node("").is_some());
    assert_eq!(grammar.resolve_anonymous_node("\0"), None);
    assert_eq!(grammar.all_anonymous_node_kinds(), [""]);
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
fn json_round_trips_raw_grammar() {
    let json = r#"{
        "name": "test",
        "rules": {
            "program": { "type": "STRING", "value": "x" }
        }
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let encoded_json = raw.to_json().unwrap();

    assert_eq!(RawGrammar::from_json(&encoded_json).unwrap(), raw);
}
