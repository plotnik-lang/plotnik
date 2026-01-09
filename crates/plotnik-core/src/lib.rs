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

pub use interner::{Interner, Symbol};

/// Raw node definition from `node-types.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawCardinality {
    pub multiple: bool,
    pub required: bool,
    pub types: Vec<RawTypeRef>,
}

/// Reference to a node type.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawTypeRef {
    #[serde(rename = "type")]
    pub type_name: String,
    pub named: bool,
}

/// Parse `node-types.json` content into raw nodes.
pub fn parse_node_types(json: &str) -> Result<Vec<RawNode>, serde_json::Error> {
    serde_json::from_str(json)
}

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
