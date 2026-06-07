use super::*;

fn find_node<'a>(
    nodes: &'a [crate::NodeShape],
    type_name: &str,
    named: bool,
) -> &'a crate::NodeShape {
    nodes
        .iter()
        .find(|node| node.type_name == type_name && node.named == named)
        .unwrap()
}

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

    let nodes = Grammar::from_json(json).unwrap().node_shapes();

    let program = find_node(&nodes, "program", true);
    assert!(program.root);
    let children = program.children.as_ref().unwrap();
    assert!(children.required);
    assert!(children.multiple);
    assert_eq!(children.types[0].type_name, "statement");

    let function = find_node(&nodes, "function", true);
    let name = function.fields.get("name").unwrap();
    assert!(name.required);
    assert!(!name.multiple);
    assert_eq!(name.types[0].type_name, "identifier");

    let body = function.fields.get("body").unwrap();
    assert_eq!(body.types[0].type_name, "block");

    let comment = find_node(&nodes, "comment", true);
    assert!(comment.extra);
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

    let nodes = Grammar::from_json(json).unwrap().node_shapes();
    let expression = find_node(&nodes, "expression", true);
    let subtypes = expression.subtypes.as_ref().unwrap();

    assert_eq!(subtypes.len(), 2);
    assert_eq!(subtypes[0].type_name, "identifier");
    assert_eq!(subtypes[1].type_name, "number");
}
