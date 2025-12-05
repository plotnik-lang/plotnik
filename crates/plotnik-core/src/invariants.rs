//! Invariant checks excluded from coverage reports.

#![cfg_attr(coverage_nightly, coverage(off))]

use crate::{DynamicNodeTypes, NodeTypeId, NodeTypeInfo, StaticNodeTypeInfo, StaticNodeTypes};

impl StaticNodeTypes {
    pub(crate) fn ensure_node(&self, node_type_id: NodeTypeId) -> &'static StaticNodeTypeInfo {
        self.get(node_type_id).unwrap_or_else(|| {
            panic!(
                "NodeTypes: node_type_id {node_type_id} not found \
                 (Lang must verify Language ↔ NodeTypes correspondence)"
            )
        })
    }
}

impl DynamicNodeTypes {
    pub(crate) fn ensure_node(&self, node_type_id: NodeTypeId) -> &NodeTypeInfo {
        self.get(node_type_id).unwrap_or_else(|| {
            panic!(
                "NodeTypes: node_type_id {node_type_id} not found \
                 (Lang must verify Language ↔ NodeTypes correspondence)"
            )
        })
    }
}
