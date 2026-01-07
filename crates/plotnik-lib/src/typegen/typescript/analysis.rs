//! Type graph traversal and analysis.

use std::collections::{HashMap, HashSet};

use crate::bytecode::{TypeData, TypeId, TypeKind};

use super::Emitter;

impl Emitter<'_> {
    pub(super) fn collect_builtin_references(&mut self) {
        for i in 0..self.entrypoints.len() {
            let ep = self.entrypoints.get(i);
            self.collect_refs_recursive(ep.result_type());
        }
    }

    fn collect_refs_recursive(&mut self, type_id: TypeId) {
        // Cycle detection
        if !self.refs_visited.insert(type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.classify() {
            TypeData::Primitive(TypeKind::Node) => {
                self.node_referenced = true;
            }
            TypeData::Primitive(_) => {}
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                self.node_referenced = true;
            }
            TypeData::Wrapper { inner, .. } => {
                self.collect_refs_recursive(inner);
            }
            TypeData::Composite { .. } => {
                let member_types: Vec<_> = self
                    .types
                    .members_of(&type_def)
                    .map(|m| m.type_id())
                    .collect();
                for ty in member_types {
                    self.collect_refs_recursive(ty);
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

        // Build dependency graph
        for &tid in &types {
            for dep in self.get_direct_deps(tid) {
                if types.contains(&dep) && dep != tid {
                    deps.entry(tid).or_default().insert(dep);
                    rdeps.entry(dep).or_default().insert(tid);
                }
            }
        }

        // Kahn's algorithm
        let mut result = Vec::with_capacity(types.len());
        let mut queue: Vec<TypeId> = deps
            .iter()
            .filter(|(_, d)| d.is_empty())
            .map(|(&tid, _)| tid)
            .collect();

        queue.sort_by_key(|tid| tid.0);

        while let Some(tid) = queue.pop() {
            result.push(tid);
            if let Some(dependents) = rdeps.get(&tid) {
                for &dependent in dependents {
                    if let Some(dep_set) = deps.get_mut(&dependent) {
                        dep_set.remove(&tid);
                        if dep_set.is_empty() {
                            queue.push(dependent);
                            queue.sort_by_key(|t| t.0);
                        }
                    }
                }
            }
        }

        result
    }

    pub(super) fn collect_reachable_types(&self, type_id: TypeId, out: &mut HashSet<TypeId>) {
        if out.contains(&type_id) {
            return;
        }

        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        match type_def.classify() {
            TypeData::Primitive(_) => {}
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => {
                out.insert(type_id);
            }
            TypeData::Wrapper { inner, .. } => {
                self.collect_reachable_types(inner, out);
            }
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            } => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_reachable_types(member.type_id(), out);
                }
            }
            TypeData::Composite {
                kind: TypeKind::Enum,
                ..
            } => {
                out.insert(type_id);
                for member in self.types.members_of(&type_def) {
                    self.collect_enum_variant_refs(member.type_id(), out);
                }
            }
            TypeData::Composite { .. } => {}
        }
    }

    /// Collect reachable types from enum variant payloads.
    /// Recurses into struct fields but doesn't add the payload struct itself.
    fn collect_enum_variant_refs(&self, type_id: TypeId, out: &mut HashSet<TypeId>) {
        let Some(type_def) = self.types.get(type_id) else {
            return;
        };

        // For struct payloads, don't add the struct itself (it will be inlined),
        // but recurse into its fields to find named types.
        if matches!(
            type_def.classify(),
            TypeData::Composite {
                kind: TypeKind::Struct,
                ..
            }
        ) {
            for member in self.types.members_of(&type_def) {
                self.collect_reachable_types(member.type_id(), out);
            }
        } else {
            // For non-struct payloads, fall back to regular collection.
            self.collect_reachable_types(type_id, out);
        }
    }

    pub(super) fn get_direct_deps(&self, type_id: TypeId) -> Vec<TypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        match type_def.classify() {
            TypeData::Primitive(_) => vec![],
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => vec![],
            TypeData::Wrapper { inner, .. } => self.unwrap_for_deps(inner),
            TypeData::Composite { .. } => self
                .types
                .members_of(&type_def)
                .flat_map(|member| self.unwrap_for_deps(member.type_id()))
                .collect(),
        }
    }

    fn unwrap_for_deps(&self, type_id: TypeId) -> Vec<TypeId> {
        let Some(type_def) = self.types.get(type_id) else {
            return vec![];
        };

        match type_def.classify() {
            TypeData::Primitive(_) => vec![],
            // Alias is a named type, so it's a dependency itself
            TypeData::Wrapper {
                kind: TypeKind::Alias,
                ..
            } => vec![type_id],
            // Other wrappers: recurse into inner type
            TypeData::Wrapper { inner, .. } => self.unwrap_for_deps(inner),
            TypeData::Composite { .. } => vec![type_id],
        }
    }
}
