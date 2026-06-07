use std::num::NonZeroU16;

use super::{
    DynamicNodeTypes, NodeKindRef, NodeShape, NodeShapeBuildError, NodeSlot, NodeTypeId, NodeTypes,
};

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
fn try_build_resolves_all_node_shape_references() {
    let shapes = vec![NodeShape {
        type_name: "root".to_string(),
        named: true,
        root: true,
        extra: false,
        fields: [(
            "body".to_string(),
            NodeSlot {
                multiple: false,
                required: true,
                types: vec![NodeKindRef {
                    type_name: "child".to_string(),
                    named: true,
                }],
            },
        )]
        .into(),
        children: Some(NodeSlot {
            multiple: true,
            required: false,
            types: vec![NodeKindRef {
                type_name: "child".to_string(),
                named: true,
            }],
        }),
        subtypes: None,
    }];

    let node_types = DynamicNodeTypes::try_build(&shapes, node_id_for_name, field_id_for_name)
        .expect("node shapes should resolve");

    assert_eq!(node_types.root(), NonZeroU16::new(1));
    assert_eq!(node_types.len(), 1);
}

#[test]
fn try_build_rejects_unknown_child_types() {
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

    let err = DynamicNodeTypes::try_build(&shapes, node_id_for_name, field_id_for_name)
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
fn try_build_skips_known_abstract_child_shapes() {
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

    let node_types = DynamicNodeTypes::try_build(&shapes, node_id_for_name, field_id_for_name)
        .expect("known abstract shapes should not be runtime node ids");
    let root_id = NonZeroU16::new(1).unwrap();

    assert_eq!(node_types.valid_child_types(root_id), &[]);
}
