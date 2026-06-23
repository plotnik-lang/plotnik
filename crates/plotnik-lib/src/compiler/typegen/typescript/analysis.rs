//! Type graph traversal and analysis.

use std::collections::{BinaryHeap, HashMap, HashSet};

use crate::bytecode::{TypeDefKind, TypeId, TypeKind};

use super::Emitter;

impl Emitter<'_> {
    pub(super) fn mark_node_reachable(&mut self) {
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.visit_for_node_use(ep.result_type());
        }
    }

    fn visit_for_node_use(&mut self, type_id: TypeId) {
        if !self.node_scan_visited.insert(type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.decode() {
            TypeDefKind::Primitive(TypeKind::Node) => {
                self.node_reachable = true;
            }
            TypeDefKind::Primitive(_) => {}
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                self.node_reachable = true;
            }
            TypeDefKind::Wrapper { inner, .. } => {
                self.visit_for_node_use(inner);
            }
            TypeDefKind::Struct { .. } | TypeDefKind::Enum { .. } => {
                let member_types: Vec<_> = self
                    .types
                    .members_of(&type_def)
                    .map(|m| m.type_id)
                    .collect();
                for ty in member_types {
                    self.visit_for_node_use(ty);
                }
            }
        }
    }

    pub(super) fn sort_topologically(&self, types: HashSet<TypeId>) -> Vec<TypeId> {
        let mut deps: HashMap<TypeId, HashSet<TypeId>> = HashMap::new();
        let mut rdeps: HashMap<TypeId, HashSet<TypeId>> = HashMap::new();

        for &tid in &types {
            deps.entry(tid).or_default();
            rdeps.entry(tid).or_default();
        }

        for &tid in &types {
            for dep in self.direct_type_deps(tid) {
                if types.contains(&dep) && dep != tid {
                    deps.entry(tid).or_default().insert(dep);
                    rdeps.entry(dep).or_default().insert(tid);
                }
            }
        }

        // Kahn's algorithm. Ready types are kept in a max-heap keyed by raw id
        // (TypeId is not Ord) so each step deterministically takes the largest
        // available id, matching the previous sort-then-pop-last ordering.
        let mut result = Vec::with_capacity(types.len());
        let mut queue: BinaryHeap<u16> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(&tid, _)| tid.0)
            .collect();

        while let Some(raw) = queue.pop() {
            let tid = TypeId(raw);
            result.push(tid);
            if let Some(dependents) = rdeps.get(&tid) {
                for &dependent in dependents {
                    if let Some(dep_set) = deps.get_mut(&dependent) {
                        dep_set.remove(&tid);
                        if dep_set.is_empty() {
                            queue.push(dependent.0);
                        }
                    }
                }
            }
        }

        result
    }

    pub(super) fn collect_emit_set(&self, type_id: TypeId, out: &mut HashSet<TypeId>) {
        if out.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.decode() {
            TypeDefKind::Primitive(_) => {}
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                out.insert(type_id);
            }
            TypeDefKind::Wrapper { inner, .. } => {
                self.collect_emit_set(inner, out);
            }
            TypeDefKind::Struct { .. } => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_emit_set(member.type_id, out);
                }
            }
            TypeDefKind::Enum { .. } => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_variant_payload_types(member.type_id, out);
                }
            }
        }
    }

    /// Collect reachable types from enum variant payloads.
    /// Recurses into struct fields but doesn't add the payload struct itself.
    fn collect_variant_payload_types(&self, type_id: TypeId, out: &mut HashSet<TypeId>) {
        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        // For struct payloads, don't add the struct itself (it will be inlined),
        // but recurse into its fields to find named types.
        if matches!(type_def.decode(), TypeDefKind::Struct { .. }) {
            for member in self.types.members_of(&type_def) {
                self.collect_emit_set(member.type_id, out);
            }
        } else {
            self.collect_emit_set(type_id, out);
        }
    }

    pub(super) fn direct_type_deps(&self, type_id: TypeId) -> Vec<TypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        match type_def.decode() {
            TypeDefKind::Primitive(_) => vec![],
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => vec![],
            TypeDefKind::Wrapper { inner, .. } => self.peel_to_named_dep(inner),
            TypeDefKind::Struct { .. } | TypeDefKind::Enum { .. } => self
                .types
                .members_of(&type_def)
                .flat_map(|member| self.peel_to_named_dep(member.type_id))
                .collect(),
        }
    }

    fn peel_to_named_dep(&self, type_id: TypeId) -> Vec<TypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        match type_def.decode() {
            TypeDefKind::Primitive(_) => vec![],
            // Alias is a named type, so it's a dependency itself
            TypeDefKind::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => vec![type_id],
            TypeDefKind::Wrapper { inner, .. } => self.peel_to_named_dep(inner),
            TypeDefKind::Struct { .. } | TypeDefKind::Enum { .. } => vec![type_id],
        }
    }
}
