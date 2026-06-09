use super::{Grammar, raw::RawGrammar};

#[test]
fn derives_root_fields_children_and_extras() {
    let json = r##"{
        "name": "test",
        "rules": {
            "program": {
                "type": "REPEAT1",
                "content": { "type": "SYMBOL", "name": "statement" }
            },
            "statement": { "type": "SYMBOL", "name": "function" },
            "function": {
                "type": "SEQ",
                "members": [
                    { "type": "FIELD", "name": "name", "content": { "type": "SYMBOL", "name": "identifier" } },
                    { "type": "FIELD", "name": "body", "content": { "type": "SYMBOL", "name": "block" } }
                ]
            },
            "identifier": { "type": "PATTERN", "value": "[a-z]+" },
            "block": { "type": "STRING", "value": "{}" },
            "comment": { "type": "PATTERN", "value": "#.*" }
        },
        "extras": [{ "type": "SYMBOL", "name": "comment" }]
    }"##;

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();

    let program = grammar.resolve_named_node("program").unwrap();
    let statement = grammar.resolve_named_node("statement").unwrap();
    assert_eq!(grammar.root(), Some(program));
    let children = grammar.children_cardinality(program).unwrap();
    assert!(children.required);
    assert!(children.multiple);
    assert!(grammar.is_valid_child_type(program, statement));

    let function = grammar.resolve_named_node("function").unwrap();
    let identifier = grammar.resolve_named_node("identifier").unwrap();
    let name_field = grammar.resolve_field("name").unwrap();
    let name = grammar.field_cardinality(function, name_field).unwrap();
    assert!(name.required);
    assert!(!name.multiple);
    assert!(grammar.is_valid_field_type(function, name_field, identifier));

    let block = grammar.resolve_named_node("block").unwrap();
    let body_field = grammar.resolve_field("body").unwrap();
    assert!(grammar.is_valid_field_type(function, body_field, block));

    let comment = grammar.resolve_named_node("comment").unwrap();
    assert!(grammar.is_extra(comment));
}

#[test]
fn derives_supertype_subtypes() {
    let json = r#"{
        "name": "test",
        "rules": {
            "program": { "type": "SYMBOL", "name": "expression" },
            "expression": {
                "type": "CHOICE",
                "members": [
                    { "type": "SYMBOL", "name": "identifier" },
                    { "type": "SYMBOL", "name": "number" }
                ]
            },
            "identifier": { "type": "PATTERN", "value": "[a-z]+" },
            "number": { "type": "PATTERN", "value": "[0-9]+" }
        },
        "supertypes": ["expression"]
    }"#;

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();
    let expression = grammar.resolve_named_node("expression").unwrap();
    let identifier = grammar.resolve_named_node("identifier").unwrap();
    let number = grammar.resolve_named_node("number").unwrap();
    let subtypes = grammar.subtypes(expression);

    assert!(grammar.is_supertype(expression));
    assert_eq!(subtypes, [identifier, number]);
}
