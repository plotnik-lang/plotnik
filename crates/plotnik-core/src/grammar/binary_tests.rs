use super::*;

#[test]
fn roundtrip() {
    let json = r#"{
        "name": "test",
        "rules": {
            "source_file": { "type": "SYMBOL", "name": "expression" },
            "expression": { "type": "STRING", "value": "x" }
        }
    }"#;

    let grammar = Grammar::from_json(json).unwrap();
    let binary = grammar.to_binary();
    let decoded = Grammar::from_binary(&binary).unwrap();

    assert_eq!(grammar.name, decoded.name);
    assert_eq!(grammar.rules.len(), decoded.rules.len());
}

#[test]
fn roundtrip_preserves_order() {
    let json = r#"{
        "name": "test",
        "rules": {
            "program": { "type": "SYMBOL", "name": "statement" },
            "statement": { "type": "SYMBOL", "name": "expression" },
            "expression": { "type": "STRING", "value": "x" }
        }
    }"#;

    let grammar = Grammar::from_json(json).unwrap();
    let binary = grammar.to_binary();
    let decoded = Grammar::from_binary(&binary).unwrap();

    assert_eq!(decoded.rules[0].0, "program");
    assert_eq!(decoded.rules[1].0, "statement");
    assert_eq!(decoded.rules[2].0, "expression");
}
