use std::num::NonZeroU16;

use super::types::{
    Grammar, GrammarMetadata, NodeKindRef, NodeShape, NodeShapeBuildError, NodeSlot, NodeSymbol,
    build_node_constraints,
};
use crate::{NodeType, NodeTypeId};

fn node_id_for_type(node_type: NodeType<&str>) -> Option<NodeTypeId> {
    match node_type {
        NodeType::Named("root") => NonZeroU16::new(1),
        NodeType::Named("child") => NonZeroU16::new(2),
        _ => None,
    }
}

fn field_id_for_name(field: &str) -> Option<NonZeroU16> {
    match field {
        "body" => NonZeroU16::new(1),
        _ => None,
    }
}

#[test]
fn builds_node_constraints_from_node_shapes() {
    let shapes = vec![NodeShape {
        type_name: "root".to_string(),
        named: true,
        root: true,
        extra: false,
        fields: [("body".to_string(), child_slot(false, true))].into(),
        children: Some(child_slot(true, false)),
        subtypes: None,
    }];

    let (node_constraints, _, root_node_type) =
        build_node_constraints(&shapes, node_id_for_type, field_id_for_name)
            .expect("node shapes should resolve");

    assert_eq!(root_node_type, NonZeroU16::new(1));
    assert_eq!(node_constraints.len(), 1);
}

#[test]
fn rejects_unknown_child_types() {
    let shapes = vec![NodeShape {
        type_name: "root".to_string(),
        named: true,
        root: true,
        extra: false,
        fields: Default::default(),
        children: Some(NodeSlot {
            multiple: true,
            required: false,
            types: vec![NodeKindRef {
                type_name: "missing".to_string(),
                named: true,
            }],
        }),
        subtypes: None,
    }];

    let err = build_node_constraints(&shapes, node_id_for_type, field_id_for_name)
        .expect_err("unknown child type should fail");

    assert_eq!(
        err,
        NodeShapeBuildError::ChildType {
            node_kind: "root".to_string(),
            kind: "missing".to_string(),
            named: true,
        }
    );
}

#[test]
fn skips_known_abstract_child_shapes() {
    let shapes = vec![
        NodeShape {
            type_name: "root".to_string(),
            named: true,
            root: true,
            extra: false,
            fields: Default::default(),
            children: Some(NodeSlot {
                multiple: true,
                required: false,
                types: vec![NodeKindRef {
                    type_name: "_abstract".to_string(),
                    named: true,
                }],
            }),
            subtypes: None,
        },
        NodeShape {
            type_name: "_abstract".to_string(),
            named: true,
            root: false,
            extra: false,
            fields: Default::default(),
            children: None,
            subtypes: None,
        },
    ];

    let (node_constraints, _, _) =
        build_node_constraints(&shapes, node_id_for_type, field_id_for_name)
            .expect("known abstract shapes should not be runtime node ids");
    let root_id = NonZeroU16::new(1).unwrap();

    let children = node_constraints[&root_id].children.as_ref().unwrap();
    assert_eq!(children.valid_types, &[]);
}

fn child_slot(multiple: bool, required: bool) -> NodeSlot {
    NodeSlot {
        multiple,
        required,
        types: vec![NodeKindRef {
            type_name: "child".to_string(),
            named: true,
        }],
    }
}

fn named_symbol(id: u16, name: &str, terminal: bool) -> NodeSymbol {
    NodeSymbol {
        id,
        type_name: name.to_string(),
        named: true,
        visible: true,
        supertype: false,
        terminal,
    }
}

fn slot_of(child: &str) -> NodeSlot {
    NodeSlot {
        multiple: true,
        required: false,
        types: vec![NodeKindRef {
            type_name: child.to_string(),
            named: true,
        }],
    }
}

fn leaf_shape(name: &str) -> NodeShape {
    NodeShape {
        type_name: name.to_string(),
        named: true,
        root: false,
        extra: false,
        fields: Default::default(),
        children: None,
        subtypes: None,
    }
}

#[test]
fn alias_token_with_children_is_not_a_leaf_token() {
    // A kind reached by a terminal symbol must not be classified as a leaf token when it also has a
    // children slot: `leafy` is reached by a terminal symbol, so the per-kind terminal accumulation
    // marks it as terminal — yet its node shape declares a real children slot. `is_token` must
    // consult the shape and return false, so a named child under it (`(leafy (child))`, a structure
    // real trees produce) is not rejected.
    let metadata = GrammarMetadata {
        node_shapes: vec![
            NodeShape {
                type_name: "root".to_string(),
                named: true,
                root: true,
                extra: false,
                fields: Default::default(),
                children: Some(slot_of("leafy")),
                subtypes: None,
            },
            NodeShape {
                type_name: "leafy".to_string(),
                named: true,
                root: false,
                extra: false,
                fields: Default::default(),
                children: Some(slot_of("child")),
                subtypes: None,
            },
            leaf_shape("child"),
        ],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "leafy", true),
            named_symbol(3, "child", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_metadata("test".to_string(), metadata).expect("metadata builds");
    let leafy = grammar
        .resolve_named_node("leafy")
        .expect("leafy is a node kind");

    assert!(
        !grammar.is_token(leafy),
        "a kind with a children slot must never be classified as a leaf token"
    );
    assert!(!grammar.valid_child_types(leafy).is_empty());
}

#[test]
fn terminal_kind_without_children_is_a_leaf_token() {
    // A genuinely childless terminal kind must still classify as a leaf token, so named children
    // under it are still rejected.
    let metadata = GrammarMetadata {
        node_shapes: vec![
            NodeShape {
                type_name: "root".to_string(),
                named: true,
                root: true,
                extra: false,
                fields: Default::default(),
                children: Some(slot_of("leaf")),
                subtypes: None,
            },
            leaf_shape("leaf"),
        ],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "leaf", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_metadata("test".to_string(), metadata).expect("metadata builds");
    let leaf = grammar
        .resolve_named_node("leaf")
        .expect("leaf is a node kind");

    assert!(grammar.is_token(leaf));
}

#[test]
fn constraint_lookup_on_shapeless_kind_is_empty_not_panic() {
    // A token-like kind can have a symbol but no node shape (e.g. typescript `jsx_text`). Constraint
    // lookups for it must return empty instead of panicking, so the linker never crashes on a query
    // such as `(jsx_text (comment))`.
    let metadata = GrammarMetadata {
        node_shapes: vec![NodeShape {
            type_name: "root".to_string(),
            named: true,
            root: true,
            extra: false,
            fields: Default::default(),
            children: None,
            subtypes: None,
        }],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "bare", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_metadata("test".to_string(), metadata).expect("metadata builds");
    let bare = grammar
        .resolve_named_node("bare")
        .expect("bare is a node kind");

    assert!(grammar.valid_child_types(bare).is_empty());
}
