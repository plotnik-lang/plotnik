#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Core data structures for Plotnik node type information.
//!
//! Two layers:
//! - **Deserialization layer**: 1:1 mapping to `node-types.json`
//! - **Analysis layer**: ID-indexed structures for efficient lookups
//!
//! Two implementations:
//! - **Dynamic** (`DynamicNodeTypes`): HashMap-based, for runtime construction
//! - **Static** (`StaticNodeTypes`): Array-based, zero runtime init

use std::collections::HashMap;
use std::num::NonZeroU16;

mod invariants;

// ============================================================================
// Deserialization Layer
// ============================================================================

/// Raw node definition from `node-types.json`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RawNode {
    #[serde(rename = "type")]
    pub type_name: String,
    pub named: bool,
    #[serde(default)]
    pub root: bool,
    #[serde(default)]
    pub extra: bool,
    #[serde(default)]
    pub fields: HashMap<String, RawCardinality>,
    pub children: Option<RawCardinality>,
    pub subtypes: Option<Vec<RawTypeRef>>,
}

/// Cardinality constraints for a field or children slot.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RawCardinality {
    pub multiple: bool,
    pub required: bool,
    pub types: Vec<RawTypeRef>,
}

/// Reference to a node type.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct RawTypeRef {
    #[serde(rename = "type")]
    pub type_name: String,
    pub named: bool,
}

/// Parse `node-types.json` content into raw nodes.
pub fn parse_node_types(json: &str) -> Result<Vec<RawNode>, serde_json::Error> {
    serde_json::from_str(json)
}

// ============================================================================
// Common Types
// ============================================================================

/// Node type ID (tree-sitter uses u16).
pub type NodeTypeId = u16;

/// Field ID (tree-sitter uses NonZeroU16).
pub type NodeFieldId = NonZeroU16;

/// Cardinality info for a field or children slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub multiple: bool,
    pub required: bool,
}

// ============================================================================
// NodeTypes Trait
// ============================================================================

/// Trait for node type constraint lookups.
///
/// Provides only what tree-sitter's `Language` API doesn't:
/// - Root node identification
/// - Extra nodes (comments, whitespace)
/// - Field constraints per node type
/// - Children constraints per node type
///
/// For name↔ID resolution and supertype info, use `Language` directly.
pub trait NodeTypes {
    fn root(&self) -> Option<NodeTypeId>;
    fn is_extra(&self, node_type_id: NodeTypeId) -> bool;

    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool;
    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality>;
    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId];
    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool;

    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality>;
    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId];
    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool;
}

impl<T: NodeTypes + ?Sized> NodeTypes for &T {
    fn root(&self) -> Option<NodeTypeId> {
        (*self).root()
    }
    fn is_extra(&self, node_type_id: NodeTypeId) -> bool {
        (*self).is_extra(node_type_id)
    }
    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool {
        (*self).has_field(node_type_id, node_field_id)
    }
    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        (*self).field_cardinality(node_type_id, node_field_id)
    }
    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId] {
        (*self).valid_field_types(node_type_id, node_field_id)
    }
    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool {
        (*self).is_valid_field_type(node_type_id, node_field_id, child)
    }
    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality> {
        (*self).children_cardinality(node_type_id)
    }
    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId] {
        (*self).valid_child_types(node_type_id)
    }
    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool {
        (*self).is_valid_child_type(node_type_id, child)
    }
}

// ============================================================================
// Static Analysis Layer (zero runtime init)
// ============================================================================

/// Field info for static storage.
#[derive(Debug, Clone, Copy)]
pub struct StaticFieldInfo {
    pub cardinality: Cardinality,
    pub valid_types: &'static [NodeTypeId],
}

/// Children info for static storage.
#[derive(Debug, Clone, Copy)]
pub struct StaticChildrenInfo {
    pub cardinality: Cardinality,
    pub valid_types: &'static [NodeTypeId],
}

/// Complete node type information for static storage.
///
/// Note: supertype/subtype info is NOT stored here - use `Language::node_kind_is_supertype()`
/// and `Language::subtypes_for_supertype()` from tree-sitter instead.
#[derive(Debug, Clone, Copy)]
pub struct StaticNodeTypeInfo {
    pub name: &'static str,
    pub named: bool,
    /// Sorted slice of (field_id, field_info) pairs for binary search.
    pub fields: &'static [(NodeFieldId, StaticFieldInfo)],
    pub children: Option<StaticChildrenInfo>,
}

/// Compiled node type database with static storage.
///
/// All data is statically allocated - no runtime initialization needed.
/// Node lookups use binary search on sorted arrays.
#[derive(Debug, Clone, Copy)]
pub struct StaticNodeTypes {
    /// Sorted slice of (node_id, node_info) pairs.
    nodes: &'static [(NodeTypeId, StaticNodeTypeInfo)],
    /// Slice of extra node type IDs.
    extras: &'static [NodeTypeId],
    root: Option<NodeTypeId>,
}

impl StaticNodeTypes {
    pub const fn new(
        nodes: &'static [(NodeTypeId, StaticNodeTypeInfo)],
        extras: &'static [NodeTypeId],
        root: Option<NodeTypeId>,
    ) -> Self {
        Self {
            nodes,
            extras,
            root,
        }
    }

    /// Get info for a node type by ID (binary search).
    pub fn get(&self, node_type_id: NodeTypeId) -> Option<&'static StaticNodeTypeInfo> {
        self.nodes
            .binary_search_by_key(&node_type_id, |(node_id, _)| *node_id)
            .ok()
            .map(|idx| &self.nodes[idx].1)
    }

    /// Check if node type exists.
    pub fn contains(&self, node_type_id: NodeTypeId) -> bool {
        self.nodes
            .binary_search_by_key(&node_type_id, |(node_id, _)| *node_id)
            .is_ok()
    }

    /// Get field info for a node type (binary search for node, then field).
    pub fn field(
        &self,
        node_type_id: NodeTypeId,
        field_id: NodeFieldId,
    ) -> Option<&'static StaticFieldInfo> {
        let info = self.ensure_node(node_type_id);
        info.fields
            .binary_search_by_key(&field_id, |(fid, _)| *fid)
            .ok()
            .map(|idx| &info.fields[idx].1)
    }

    /// Get children info for a node type.
    pub fn children(&self, node_type_id: NodeTypeId) -> Option<StaticChildrenInfo> {
        self.ensure_node(node_type_id).children
    }

    /// Get all extra node type IDs.
    pub fn extras(&self) -> &'static [NodeTypeId] {
        self.extras
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeTypeId, &'static StaticNodeTypeInfo)> {
        self.nodes.iter().map(|(id, info)| (*id, info))
    }
}

impl NodeTypes for StaticNodeTypes {
    fn root(&self) -> Option<NodeTypeId> {
        self.root
    }

    fn is_extra(&self, node_type_id: NodeTypeId) -> bool {
        self.extras.contains(&node_type_id)
    }

    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool {
        self.get(node_type_id).is_some_and(|info| {
            info.fields
                .binary_search_by_key(&node_field_id, |(fid, _)| *fid)
                .is_ok()
        })
    }

    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.field(node_type_id, node_field_id)
            .map(|f| f.cardinality)
    }

    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId] {
        self.field(node_type_id, node_field_id)
            .map(|f| f.valid_types)
            .unwrap_or(&[])
    }

    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool {
        self.valid_field_types(node_type_id, node_field_id)
            .contains(&child)
    }

    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality> {
        self.children(node_type_id).map(|c| c.cardinality)
    }

    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId] {
        self.children(node_type_id)
            .map(|c| c.valid_types)
            .unwrap_or(&[])
    }

    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool {
        self.valid_child_types(node_type_id).contains(&child)
    }
}

// ============================================================================
// Dynamic Analysis Layer (runtime construction)
// ============================================================================

/// Information about a single field on a node type.
#[derive(Debug, Clone)]
pub struct FieldInfo {
    pub cardinality: Cardinality,
    pub valid_types: Vec<NodeTypeId>,
}

/// Information about a node type's children (non-field children).
#[derive(Debug, Clone)]
pub struct ChildrenInfo {
    pub cardinality: Cardinality,
    pub valid_types: Vec<NodeTypeId>,
}

/// Complete node type information.
///
/// Note: supertype/subtype info is NOT stored here - use tree-sitter's Language API.
#[derive(Debug, Clone)]
pub struct NodeTypeInfo {
    pub name: String,
    pub named: bool,
    pub fields: HashMap<NodeFieldId, FieldInfo>,
    pub children: Option<ChildrenInfo>,
}

/// Compiled node type database for a language (dynamic/heap-allocated).
///
/// Use this for runtime construction or as reference implementation.
/// For zero-init static data, use `StaticNodeTypes`.
#[derive(Debug, Clone)]
pub struct DynamicNodeTypes {
    nodes: HashMap<NodeTypeId, NodeTypeInfo>,
    extras: Vec<NodeTypeId>,
    root: Option<NodeTypeId>,
}

impl DynamicNodeTypes {
    pub fn from_raw(
        nodes: HashMap<NodeTypeId, NodeTypeInfo>,
        extras: Vec<NodeTypeId>,
        root: Option<NodeTypeId>,
    ) -> Self {
        Self {
            nodes,
            extras,
            root,
        }
    }

    /// Build from raw nodes and ID resolution functions.
    pub fn build<F, G>(raw_nodes: &[RawNode], node_id_for_name: F, field_id_for_name: G) -> Self
    where
        F: Fn(&str, bool) -> Option<NodeTypeId>,
        G: Fn(&str) -> Option<NodeFieldId>,
    {
        let mut nodes = HashMap::new();
        let mut extras = Vec::new();
        let mut root = None;

        for raw in raw_nodes {
            let Some(node_id) = node_id_for_name(&raw.type_name, raw.named) else {
                continue;
            };

            if raw.root {
                root = Some(node_id);
            }

            if raw.extra {
                extras.push(node_id);
            }

            let mut fields = HashMap::new();
            for (field_name, raw_card) in &raw.fields {
                let Some(field_id) = field_id_for_name(field_name) else {
                    continue;
                };

                let valid_types = raw_card
                    .types
                    .iter()
                    .filter_map(|t| node_id_for_name(&t.type_name, t.named))
                    .collect();

                fields.insert(
                    field_id,
                    FieldInfo {
                        cardinality: Cardinality {
                            multiple: raw_card.multiple,
                            required: raw_card.required,
                        },
                        valid_types,
                    },
                );
            }

            let children = raw.children.as_ref().map(|raw_card| {
                let valid_types = raw_card
                    .types
                    .iter()
                    .filter_map(|t| node_id_for_name(&t.type_name, t.named))
                    .collect();

                ChildrenInfo {
                    cardinality: Cardinality {
                        multiple: raw_card.multiple,
                        required: raw_card.required,
                    },
                    valid_types,
                }
            });

            nodes.insert(
                node_id,
                NodeTypeInfo {
                    name: raw.type_name.clone(),
                    named: raw.named,
                    fields,
                    children,
                },
            );
        }

        Self {
            nodes,
            extras,
            root,
        }
    }

    pub fn get(&self, node_type_id: NodeTypeId) -> Option<&NodeTypeInfo> {
        self.nodes.get(&node_type_id)
    }

    pub fn contains(&self, node_type_id: NodeTypeId) -> bool {
        self.nodes.contains_key(&node_type_id)
    }

    pub fn field(&self, node_type_id: NodeTypeId, field_id: NodeFieldId) -> Option<&FieldInfo> {
        self.ensure_node(node_type_id).fields.get(&field_id)
    }

    pub fn children(&self, node_type_id: NodeTypeId) -> Option<&ChildrenInfo> {
        self.ensure_node(node_type_id).children.as_ref()
    }

    pub fn extras(&self) -> &[NodeTypeId] {
        &self.extras
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (NodeTypeId, &NodeTypeInfo)> {
        self.nodes.iter().map(|(&id, info)| (id, info))
    }

    /// Get sorted vec of all node IDs (for conversion to static).
    pub fn sorted_node_ids(&self) -> Vec<NodeTypeId> {
        let mut ids: Vec<_> = self.nodes.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Get sorted vec of extra IDs (for conversion to static).
    pub fn sorted_extras(&self) -> Vec<NodeTypeId> {
        let mut ids = self.extras.clone();
        ids.sort_unstable();
        ids
    }
}

impl NodeTypes for DynamicNodeTypes {
    fn root(&self) -> Option<NodeTypeId> {
        self.root
    }

    fn is_extra(&self, node_type_id: NodeTypeId) -> bool {
        self.extras.contains(&node_type_id)
    }

    fn has_field(&self, node_type_id: NodeTypeId, node_field_id: NodeFieldId) -> bool {
        self.nodes
            .get(&node_type_id)
            .is_some_and(|n| n.fields.contains_key(&node_field_id))
    }

    fn field_cardinality(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> Option<Cardinality> {
        self.field(node_type_id, node_field_id)
            .map(|f| f.cardinality)
    }

    fn valid_field_types(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
    ) -> &[NodeTypeId] {
        self.field(node_type_id, node_field_id)
            .map(|f| f.valid_types.as_slice())
            .unwrap_or(&[])
    }

    fn is_valid_field_type(
        &self,
        node_type_id: NodeTypeId,
        node_field_id: NodeFieldId,
        child: NodeTypeId,
    ) -> bool {
        self.valid_field_types(node_type_id, node_field_id)
            .contains(&child)
    }

    fn children_cardinality(&self, node_type_id: NodeTypeId) -> Option<Cardinality> {
        self.children(node_type_id).map(|c| c.cardinality)
    }

    fn valid_child_types(&self, node_type_id: NodeTypeId) -> &[NodeTypeId] {
        self.children(node_type_id)
            .map(|c| c.valid_types.as_slice())
            .unwrap_or(&[])
    }

    fn is_valid_child_type(&self, node_type_id: NodeTypeId, child: NodeTypeId) -> bool {
        self.valid_child_types(node_type_id).contains(&child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn build_dynamic_node_types() {
        let raw = parse_node_types(SAMPLE_JSON).unwrap();

        let node_ids: HashMap<(&str, bool), NodeTypeId> = [
            (("expression", true), 1),
            (("function_declaration", true), 2),
            (("program", true), 3),
            (("comment", true), 4),
            (("identifier", true), 5),
            (("+", false), 6),
            (("block", true), 7),
            (("statement", true), 8),
            (("number", true), 9),
        ]
        .into_iter()
        .collect();

        let field_ids: HashMap<&str, NodeFieldId> = [
            ("name", NonZeroU16::new(1).unwrap()),
            ("body", NonZeroU16::new(2).unwrap()),
        ]
        .into_iter()
        .collect();

        let node_types = DynamicNodeTypes::build(
            &raw,
            |name, named| node_ids.get(&(name, named)).copied(),
            |name| field_ids.get(name).copied(),
        );

        assert_eq!(node_types.len(), 6);

        // Test via trait
        assert_eq!(node_types.root(), Some(3));
        assert!(node_types.is_extra(4));
        assert!(!node_types.is_extra(5));
        assert!(node_types.has_field(2, NonZeroU16::new(1).unwrap()));
        assert!(node_types.has_field(2, NonZeroU16::new(2).unwrap()));
        assert!(!node_types.has_field(2, NonZeroU16::new(99).unwrap()));
        assert!(node_types.is_valid_field_type(2, NonZeroU16::new(1).unwrap(), 5));
        assert!(!node_types.is_valid_field_type(2, NonZeroU16::new(1).unwrap(), 7));
    }

    // Static tests using manually constructed data
    static TEST_VALID_TYPES_ID: [NodeTypeId; 1] = [5]; // identifier
    static TEST_VALID_TYPES_BLOCK: [NodeTypeId; 1] = [7]; // block
    static TEST_CHILDREN_TYPES: [NodeTypeId; 1] = [8]; // statement

    static TEST_FIELDS: [(NodeFieldId, StaticFieldInfo); 2] = [
        (
            NonZeroU16::new(1).unwrap(),
            StaticFieldInfo {
                cardinality: Cardinality {
                    multiple: false,
                    required: true,
                },
                valid_types: &TEST_VALID_TYPES_ID,
            },
        ),
        (
            NonZeroU16::new(2).unwrap(),
            StaticFieldInfo {
                cardinality: Cardinality {
                    multiple: false,
                    required: true,
                },
                valid_types: &TEST_VALID_TYPES_BLOCK,
            },
        ),
    ];

    static TEST_NODES: [(NodeTypeId, StaticNodeTypeInfo); 4] = [
        (
            1,
            StaticNodeTypeInfo {
                name: "expression",
                named: true,
                fields: &[],
                children: None,
            },
        ),
        (
            2,
            StaticNodeTypeInfo {
                name: "function_declaration",
                named: true,
                fields: &TEST_FIELDS,
                children: None,
            },
        ),
        (
            3,
            StaticNodeTypeInfo {
                name: "program",
                named: true,
                fields: &[],
                children: Some(StaticChildrenInfo {
                    cardinality: Cardinality {
                        multiple: true,
                        required: false,
                    },
                    valid_types: &TEST_CHILDREN_TYPES,
                }),
            },
        ),
        (
            4,
            StaticNodeTypeInfo {
                name: "comment",
                named: true,
                fields: &[],
                children: None,
            },
        ),
    ];

    static TEST_EXTRAS: [NodeTypeId; 1] = [4];

    static TEST_STATIC_NODE_TYPES: StaticNodeTypes =
        StaticNodeTypes::new(&TEST_NODES, &TEST_EXTRAS, Some(3));

    #[test]
    fn static_node_types_get() {
        let info = TEST_STATIC_NODE_TYPES.get(2).unwrap();
        assert_eq!(info.name, "function_declaration");
        assert!(info.named);

        assert!(TEST_STATIC_NODE_TYPES.get(99).is_none());
    }

    #[test]
    fn static_node_types_contains() {
        assert!(TEST_STATIC_NODE_TYPES.contains(1));
        assert!(TEST_STATIC_NODE_TYPES.contains(2));
        assert!(!TEST_STATIC_NODE_TYPES.contains(99));
    }

    #[test]
    fn static_node_types_trait() {
        // Test via trait methods
        assert_eq!(TEST_STATIC_NODE_TYPES.root(), Some(3));
        assert!(TEST_STATIC_NODE_TYPES.is_extra(4));
        assert!(!TEST_STATIC_NODE_TYPES.is_extra(1));

        assert!(TEST_STATIC_NODE_TYPES.has_field(2, NonZeroU16::new(1).unwrap()));
        assert!(TEST_STATIC_NODE_TYPES.has_field(2, NonZeroU16::new(2).unwrap()));
        assert!(!TEST_STATIC_NODE_TYPES.has_field(2, NonZeroU16::new(99).unwrap()));
        assert!(!TEST_STATIC_NODE_TYPES.has_field(1, NonZeroU16::new(1).unwrap()));

        assert!(TEST_STATIC_NODE_TYPES.is_valid_field_type(2, NonZeroU16::new(1).unwrap(), 5));
        assert!(!TEST_STATIC_NODE_TYPES.is_valid_field_type(2, NonZeroU16::new(1).unwrap(), 7));
        assert!(TEST_STATIC_NODE_TYPES.is_valid_field_type(2, NonZeroU16::new(2).unwrap(), 7));

        let field_types = TEST_STATIC_NODE_TYPES.valid_field_types(2, NonZeroU16::new(1).unwrap());
        assert_eq!(field_types, &[5]);

        let card = TEST_STATIC_NODE_TYPES
            .field_cardinality(2, NonZeroU16::new(1).unwrap())
            .unwrap();
        assert!(!card.multiple);
        assert!(card.required);
    }

    #[test]
    fn static_node_types_children() {
        let card = TEST_STATIC_NODE_TYPES.children_cardinality(3).unwrap();
        assert!(card.multiple);
        assert!(!card.required);

        let child_types = TEST_STATIC_NODE_TYPES.valid_child_types(3);
        assert_eq!(child_types, &[8]);

        assert!(TEST_STATIC_NODE_TYPES.is_valid_child_type(3, 8));
        assert!(!TEST_STATIC_NODE_TYPES.is_valid_child_type(3, 5));

        assert!(TEST_STATIC_NODE_TYPES.children_cardinality(1).is_none());
        assert!(TEST_STATIC_NODE_TYPES.valid_child_types(1).is_empty());
    }

    #[test]
    fn static_node_types_len() {
        assert_eq!(TEST_STATIC_NODE_TYPES.len(), 4);
        assert!(!TEST_STATIC_NODE_TYPES.is_empty());
    }

    #[test]
    fn static_node_types_iter() {
        let ids: Vec<_> = TEST_STATIC_NODE_TYPES.iter().map(|(id, _)| id).collect();
        assert_eq!(ids, vec![1, 2, 3, 4]);
    }
}
