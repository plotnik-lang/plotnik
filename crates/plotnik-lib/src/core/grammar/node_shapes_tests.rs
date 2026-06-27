use super::{Grammar, raw::RawGrammar};
use indoc::indoc;

#[test]
fn valid_child_types_reach_through_hidden_frontier() {
    // `module`'s only child is the hidden `_statement`, which fans out through another
    // hidden rule (`_simple_statements`) and a supertype (`_simple_statement`) to the
    // concrete `pass_statement` — the shape python's `(module (pass_statement))` exercises.
    // The node-shape summary flattened that chain to nothing, falsely rejecting the bare
    // child; deriving children from the structural skeleton reaches it.
    let json = indoc! {r##"
        {
            "name": "pylike",
            "rules": {
                "module": { "type": "REPEAT1", "content": { "type": "SYMBOL", "name": "_statement" } },
                "_statement": { "type": "CHOICE", "members": [
                    { "type": "SYMBOL", "name": "_simple_statements" },
                    { "type": "SYMBOL", "name": "_compound_statement" }
                ]},
                "_simple_statements": { "type": "SEQ", "members": [
                    { "type": "SYMBOL", "name": "_simple_statement" },
                    { "type": "REPEAT", "content": { "type": "SEQ", "members": [
                        { "type": "STRING", "value": ";" },
                        { "type": "SYMBOL", "name": "_simple_statement" }
                    ]}},
                    { "type": "SYMBOL", "name": "_newline" }
                ]},
                "_simple_statement": { "type": "CHOICE", "members": [
                    { "type": "SYMBOL", "name": "pass_statement" },
                    { "type": "SYMBOL", "name": "expression_statement" }
                ]},
                "_compound_statement": { "type": "CHOICE", "members": [
                    { "type": "SYMBOL", "name": "if_statement" }
                ]},
                "pass_statement": { "type": "STRING", "value": "pass" },
                "expression_statement": { "type": "SYMBOL", "name": "identifier" },
                "if_statement": { "type": "SEQ", "members": [
                    { "type": "STRING", "value": "if" }, { "type": "SYMBOL", "name": "identifier" }
                ]},
                "identifier": { "type": "PATTERN", "value": "[a-z]+" }
            },
            "extras": [],
            "externals": [{ "type": "SYMBOL", "name": "_newline" }],
            "inline": ["_simple_statement", "_compound_statement"],
            "supertypes": ["_simple_statement", "_compound_statement"]
        }
    "##};
    let grammar = Grammar::from_raw(&RawGrammar::from_json(json).unwrap()).unwrap();
    let module = grammar.resolve_named_node("module").unwrap();
    let pass = grammar.resolve_named_node("pass_statement").unwrap();

    let children = grammar.valid_child_types(module);
    assert!(
        !children.is_empty(),
        "module lost its children across the hidden frontier"
    );

    // `pass_statement` is admissible: it is a (transitive) subtype of one of module's
    // children, exactly as `admissible_set` expands them during checking.
    let reaches_pass = children
        .iter()
        .any(|&c| c == pass || grammar.collect_subtypes(c).contains(&pass));
    assert!(
        reaches_pass,
        "pass_statement must be admissible under module, got {:?}",
        children
            .iter()
            .filter_map(|&id| grammar.node_kind(id))
            .collect::<Vec<_>>()
    );
}

#[test]
fn derives_root_fields_children_and_extras() {
    let json = indoc! {r##"
        {
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
        }
    "##};

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();

    let program = grammar.resolve_named_node("program").unwrap();
    let statement = grammar.resolve_named_node("statement").unwrap();
    assert_eq!(grammar.root(), Some(program));
    let children = grammar.children_cardinality(program).unwrap();
    assert!(children.is_required());
    assert!(children.is_multiple());
    assert!(grammar.is_valid_child_type(program, statement));

    let function = grammar.resolve_named_node("function").unwrap();
    let identifier = grammar.resolve_named_node("identifier").unwrap();
    let name_field = grammar.resolve_field("name").unwrap();
    let name = grammar.field_cardinality(function, name_field).unwrap();
    assert!(name.is_required());
    assert!(!name.is_multiple());
    assert!(grammar.is_valid_field_type(function, name_field, identifier));

    let block = grammar.resolve_named_node("block").unwrap();
    let body_field = grammar.resolve_field("body").unwrap();
    assert!(grammar.is_valid_field_type(function, body_field, block));

    let comment = grammar.resolve_named_node("comment").unwrap();
    assert!(grammar.is_extra(comment));
}

#[test]
fn derives_supertype_subtypes() {
    let json = indoc! {r#"
        {
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
        }
    "#};

    let raw = RawGrammar::from_json(json).unwrap();
    let grammar = Grammar::from_raw(&raw).unwrap();
    let expression = grammar.resolve_named_node("expression").unwrap();
    let identifier = grammar.resolve_named_node("identifier").unwrap();
    let number = grammar.resolve_named_node("number").unwrap();
    let subtypes = grammar.subtypes(expression);

    assert!(grammar.is_supertype(expression));
    assert_eq!(subtypes, [identifier, number]);
}
