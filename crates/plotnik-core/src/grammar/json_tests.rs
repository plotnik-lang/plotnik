use super::*;

#[test]
fn parse_minimal_grammar() {
    let json = r#"{
        "name": "test",
        "rules": {
            "source_file": { "type": "SYMBOL", "name": "expression" },
            "expression": { "type": "STRING", "value": "x" }
        }
    }"#;

    let grammar = Grammar::from_json(json).unwrap();
    assert_eq!(grammar.name, "test");
    assert_eq!(grammar.rules.len(), 2);
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

    let grammar = Grammar::from_json(json).unwrap();
    assert!(matches!(grammar.rules[0].1, Rule::Seq(_)));
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
            }
        }
    }"#;

    let grammar = Grammar::from_json(json).unwrap();
    assert!(matches!(grammar.rules[0].1, Rule::Field { .. }));
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

    let grammar = Grammar::from_json(json).unwrap();

    // Entry rule should be first (program), not alphabetically sorted
    assert_eq!(grammar.rules[0].0, "program");
    assert_eq!(grammar.rules[1].0, "statement");
    assert_eq!(grammar.rules[2].0, "expression");
}
