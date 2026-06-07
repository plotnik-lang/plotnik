#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

//! Core data structures for Plotnik grammar-derived node information.
//!
//! Two layers:
//! - **Grammar layer**: derived from tree-sitter `grammar.json`
//! - **Lookup layer**: ID-indexed structures for efficient runtime checks
//!
//! Two implementations:
//! - **Dynamic** (`DynamicNodeTypes`): HashMap-based, for runtime construction
//! - **Static** (`StaticNodeTypes`): Array-based, zero runtime init

use std::collections::{HashMap, HashSet};
use std::num::NonZeroU16;

pub mod colors;
pub mod grammar;
mod interner;
mod invariants;
pub mod utils;

#[cfg(test)]
mod interner_tests;
#[cfg(test)]
mod lib_tests;
#[cfg(test)]
mod utils_tests;

pub use colors::Colors;
pub use interner::{Interner, Symbol};

/// Grammar-derived metadata for a syntax node kind.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeShape {
    #[serde(rename = "type")]
    pub type_name: String,
    pub named: bool,
    #[serde(default)]
    pub root: bool,
    #[serde(default)]
    pub extra: bool,
    #[serde(default)]
    pub fields: HashMap<String, NodeSlot>,
    pub children: Option<NodeSlot>,
    pub subtypes: Option<Vec<NodeKindRef>>,
}

/// Cardinality constraints for a field or children slot.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeSlot {
    pub multiple: bool,
    pub required: bool,
    pub types: Vec<NodeKindRef>,
}

/// Reference to a node kind.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeKindRef {
    #[serde(rename = "type")]
    pub type_name: String,
    pub named: bool,
}

/// Error while resolving grammar-derived node shapes against a tree-sitter language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeShapeBuildError {
    UnknownField {
        node_kind: String,
        field: String,
    },
    UnknownFieldType {
        node_kind: String,
        field: String,
        kind: String,
        named: bool,
    },
    UnknownChildType {
        node_kind: String,
        kind: String,
        named: bool,
    },
}

impl std::fmt::Display for NodeShapeBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownField { node_kind, field } => {
                write!(f, "unknown field {field:?} on node kind {node_kind:?}")
            }
            Self::UnknownFieldType {
                node_kind,
                field,
                kind,
                named,
            } => write!(
                f,
                "unknown field type {kind:?} (named: {named}) for field {field:?} on node kind {node_kind:?}"
            ),
            Self::UnknownChildType {
                node_kind,
                kind,
                named,
            } => write!(
                f,
                "unknown child type {kind:?} (named: {named}) for node kind {node_kind:?}"
            ),
        }
    }
}

impl std::error::Error for NodeShapeBuildError {}

/// Node type ID (tree-sitter uses u16, but 0 is internal-only).
pub type NodeTypeId = NonZeroU16;

/// Field ID (tree-sitter uses NonZeroU16).
pub type NodeFieldId = NonZeroU16;

/// Cardinality info for a field or children slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cardinality {
    pub multiple: bool,
    pub required: bool,
}

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

fn resolve_slot_types<F, E>(
    slot: &NodeSlot,
    known_shapes: &HashSet<(&str, bool)>,
    node_id_for_name: &F,
    error: E,
) -> Result<Vec<NodeTypeId>, NodeShapeBuildError>
where
    F: Fn(&str, bool) -> Option<NodeTypeId>,
    E: Fn(&NodeKindRef) -> NodeShapeBuildError,
{
    let mut resolved = Vec::new();
    for kind_ref in &slot.types {
        if let Some(node_id) = node_id_for_name(&kind_ref.type_name, kind_ref.named) {
            resolved.push(node_id);
            continue;
        }

        if known_shapes.contains(&(kind_ref.type_name.as_str(), kind_ref.named)) {
            continue;
        }

        Err(error(kind_ref))?;
    }

    Ok(resolved)
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

    /// Build from grammar-derived node shapes and ID resolution functions.
    pub fn try_build<F, G>(
        node_shapes: &[NodeShape],
        node_id_for_name: F,
        field_id_for_name: G,
    ) -> Result<Self, NodeShapeBuildError>
    where
        F: Fn(&str, bool) -> Option<NodeTypeId>,
        G: Fn(&str) -> Option<NodeFieldId>,
    {
        let mut nodes = HashMap::new();
        let mut extras = Vec::new();
        let mut root = None;
        let known_shapes = node_shapes
            .iter()
            .map(|shape| (shape.type_name.as_str(), shape.named))
            .collect::<HashSet<_>>();

        for shape in node_shapes {
            let Some(node_id) = node_id_for_name(&shape.type_name, shape.named) else {
                continue;
            };

            if shape.root {
                root = Some(node_id);
            }

            if shape.extra {
                extras.push(node_id);
            }

            let mut fields = HashMap::new();
            for (field_name, slot) in &shape.fields {
                let field_id = field_id_for_name(field_name).ok_or_else(|| {
                    NodeShapeBuildError::UnknownField {
                        node_kind: shape.type_name.clone(),
                        field: field_name.clone(),
                    }
                })?;

                let valid_types =
                    resolve_slot_types(slot, &known_shapes, &node_id_for_name, |kind_ref| {
                        NodeShapeBuildError::UnknownFieldType {
                            node_kind: shape.type_name.clone(),
                            field: field_name.clone(),
                            kind: kind_ref.type_name.clone(),
                            named: kind_ref.named,
                        }
                    })?;

                fields.insert(
                    field_id,
                    FieldInfo {
                        cardinality: Cardinality {
                            multiple: slot.multiple,
                            required: slot.required,
                        },
                        valid_types,
                    },
                );
            }

            let children = shape
                .children
                .as_ref()
                .map(|slot| {
                    let valid_types =
                        resolve_slot_types(slot, &known_shapes, &node_id_for_name, |kind_ref| {
                            NodeShapeBuildError::UnknownChildType {
                                node_kind: shape.type_name.clone(),
                                kind: kind_ref.type_name.clone(),
                                named: kind_ref.named,
                            }
                        })?;

                    Ok(ChildrenInfo {
                        cardinality: Cardinality {
                            multiple: slot.multiple,
                            required: slot.required,
                        },
                        valid_types,
                    })
                })
                .transpose()?;

            nodes.insert(
                node_id,
                NodeTypeInfo {
                    name: shape.type_name.clone(),
                    named: shape.named,
                    fields,
                    children,
                },
            );
        }

        Ok(Self {
            nodes,
            extras,
            root,
        })
    }

    /// Build from node shapes that have already been checked against a language.
    pub fn build<F, G>(node_shapes: &[NodeShape], node_id_for_name: F, field_id_for_name: G) -> Self
    where
        F: Fn(&str, bool) -> Option<NodeTypeId>,
        G: Fn(&str) -> Option<NodeFieldId>,
    {
        Self::try_build(node_shapes, node_id_for_name, field_id_for_name)
            .expect("grammar-derived node shapes should resolve against tree-sitter language")
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
