use super::types::{
    FieldEntry, Grammar, GrammarTables, NodeKindEntry, NodeKindRef, NodeShape, NodeShapeBuildError,
    NodeSlot, build_node_constraints,
};
use crate::core::{Cardinality, NodeFieldId, NodeKind, NodeKindId};

impl NodeShape {
    /// A named node kind: non-root, no fields, no children.
    fn named(name: &str) -> Self {
        Self {
            kind_name: name.to_string(),
            named: true,
            root: false,
            extra: false,
            fields: Default::default(),
            children: None,
            subtypes: None,
        }
    }

    /// The grammar's root node kind.
    fn root(name: &str) -> Self {
        Self {
            root: true,
            ..Self::named(name)
        }
    }

    fn with_children(mut self, children: NodeSlot) -> Self {
        self.children = Some(children);
        self
    }

    fn with_field(mut self, name: &str, slot: NodeSlot) -> Self {
        self.fields.insert(name.to_string(), slot);
        self
    }
}

fn node_id_for_type(node_kind: NodeKind<&str>) -> Option<NodeKindId> {
    match node_kind {
        NodeKind::Named("root") => NodeKindId::try_from(1).ok(),
        NodeKind::Named("child") => NodeKindId::try_from(2).ok(),
        _ => None,
    }
}

fn field_id_for_name(field: &str) -> Option<NodeFieldId> {
    match field {
        "body" => NodeFieldId::try_from(1).ok(),
        _ => None,
    }
}

#[test]
fn builds_node_constraints_from_node_shapes() {
    let shapes = vec![
        NodeShape::root("root")
            .with_field("body", child_slot(Cardinality::ExactlyOne))
            .with_children(child_slot(Cardinality::ZeroOrMore)),
    ];

    let (node_constraints, _, root_node_kind) =
        build_node_constraints(&shapes, node_id_for_type, field_id_for_name)
            .expect("node shapes should resolve");

    assert_eq!(root_node_kind, NodeKindId::try_from(1).ok());
    assert_eq!(node_constraints.len(), 1);
}

#[test]
fn rejects_unknown_child_types() {
    let shapes = vec![NodeShape::root("root").with_children(slot_of("missing"))];

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
        NodeShape::root("root").with_children(slot_of("_abstract")),
        NodeShape::named("_abstract"),
    ];

    let (node_constraints, _, _) =
        build_node_constraints(&shapes, node_id_for_type, field_id_for_name)
            .expect("known abstract shapes should not be runtime node ids");
    let root_id = NodeKindId::try_from(1).unwrap();

    let children = node_constraints[&root_id].children.as_ref().unwrap();
    assert_eq!(children.valid_types, &[]);
}

fn child_slot(cardinality: Cardinality) -> NodeSlot {
    NodeSlot {
        multiple: cardinality.is_multiple(),
        required: cardinality.is_required(),
        types: vec![NodeKindRef {
            kind_name: "child".to_string(),
            named: true,
        }],
    }
}

fn named_symbol(id: u16, name: &str, terminal: bool) -> NodeKindEntry {
    NodeKindEntry {
        id,
        kind_name: name.to_string(),
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
            kind_name: child.to_string(),
            named: true,
        }],
    }
}

#[test]
fn alias_token_with_children_is_not_a_leaf_token() {
    // A kind reached by a terminal symbol must not be classified as a leaf token when it also has a
    // children slot: `leafy` is reached by a terminal symbol, so the per-kind terminal accumulation
    // marks it as terminal — yet its node shape declares a real children slot. `is_token` must
    // consult the shape and return false, so a named child under it (`(leafy (child))`, a structure
    // real trees produce) is not rejected.
    let metadata = GrammarTables {
        node_shapes: vec![
            NodeShape::root("root").with_children(slot_of("leafy")),
            NodeShape::named("leafy").with_children(slot_of("child")),
            NodeShape::named("child"),
        ],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "leafy", true),
            named_symbol(3, "child", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_tables("test".to_string(), metadata).expect("metadata builds");
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
    let metadata = GrammarTables {
        node_shapes: vec![
            NodeShape::root("root").with_children(slot_of("leaf")),
            NodeShape::named("leaf"),
        ],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "leaf", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_tables("test".to_string(), metadata).expect("metadata builds");
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
    let metadata = GrammarTables {
        node_shapes: vec![NodeShape::root("root")],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "bare", true),
        ],
        fields: Vec::new(),
    };

    let grammar = Grammar::from_tables("test".to_string(), metadata).expect("metadata builds");
    let bare = grammar
        .resolve_named_node("bare")
        .expect("bare is a node kind");

    assert!(grammar.valid_child_types(bare).is_empty());
}

#[test]
fn declared_child_structure_uses_exact_node_shape() {
    let metadata = GrammarTables {
        node_shapes: vec![
            NodeShape::root("root").with_children(slot_of("fielded")),
            NodeShape::named("fielded").with_field("body", child_slot(Cardinality::Optional)),
            NodeShape::named("abstract_only").with_children(slot_of("_abstract")),
            NodeShape::named("_abstract"),
            NodeShape::named("leaf"),
            NodeShape::named("child"),
        ],
        symbols: vec![
            named_symbol(1, "root", false),
            named_symbol(2, "fielded", false),
            named_symbol(3, "abstract_only", false),
            named_symbol(4, "_abstract", false),
            named_symbol(5, "leaf", true),
            named_symbol(6, "child", true),
        ],
        fields: vec![FieldEntry {
            id: 1,
            name: "body".to_string(),
        }],
    };

    let grammar = Grammar::from_tables("test".to_string(), metadata).expect("metadata builds");
    let fielded = grammar.resolve_named_node("fielded").unwrap();
    let abstract_only = grammar.resolve_named_node("abstract_only").unwrap();
    let leaf = grammar.resolve_named_node("leaf").unwrap();

    assert!(grammar.has_declared_child_structure(fielded));
    assert!(grammar.has_declared_child_structure(abstract_only));
    assert!(!grammar.has_declared_child_structure(leaf));
}
