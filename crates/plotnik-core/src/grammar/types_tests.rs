use std::num::NonZeroU16;

use super::types::{NodeKindRef, NodeShape, NodeShapeBuildError, NodeSlot, build_node_constraints};
use crate::NodeTypeId;

fn node_id_for_name(kind: &str, named: bool) -> Option<NodeTypeId> {
    match (kind, named) {
        ("root", true) => NonZeroU16::new(1),
        ("child", true) => NonZeroU16::new(2),
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
        build_node_constraints(&shapes, node_id_for_name, field_id_for_name)
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

    let err = build_node_constraints(&shapes, node_id_for_name, field_id_for_name)
        .expect_err("unknown child type should fail");

    assert_eq!(
        err,
        NodeShapeBuildError::UnknownChildType {
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
        build_node_constraints(&shapes, node_id_for_name, field_id_for_name)
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
