use crate::parse_node_types;

const SAMPLE_JSON: &str = r#"[
    {
        "type": "expression",
        "named": true,
        "subtypes": [
            {"type": "identifier", "named": true},
            {"type": "number", "named": true}
        ]
    },
    {
        "type": "function_declaration",
        "named": true,
        "fields": {
            "name": {
                "multiple": false,
                "required": true,
                "types": [{"type": "identifier", "named": true}]
            },
            "body": {
                "multiple": false,
                "required": true,
                "types": [{"type": "block", "named": true}]
            }
        }
    },
    {
        "type": "program",
        "named": true,
        "root": true,
        "fields": {},
        "children": {
            "multiple": true,
            "required": false,
            "types": [{"type": "statement", "named": true}]
        }
    },
    {
        "type": "comment",
        "named": true,
        "extra": true
    },
    {
        "type": "identifier",
        "named": true
    },
    {
        "type": "+",
        "named": false
    }
]"#;

#[test]
fn parse_raw_nodes() {
    let nodes = parse_node_types(SAMPLE_JSON).unwrap();
    assert_eq!(nodes.len(), 6);

    let expr = nodes.iter().find(|n| n.type_name == "expression").unwrap();
    assert!(expr.named);
    assert!(expr.subtypes.is_some());
    assert_eq!(expr.subtypes.as_ref().unwrap().len(), 2);

    let func = nodes
        .iter()
        .find(|n| n.type_name == "function_declaration")
        .unwrap();
    assert!(func.fields.contains_key("name"));
    assert!(func.fields.contains_key("body"));

    let plus = nodes.iter().find(|n| n.type_name == "+").unwrap();
    assert!(!plus.named);
}
