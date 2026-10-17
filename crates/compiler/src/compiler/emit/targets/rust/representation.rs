use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{TypeId, TypeShape};

pub(super) struct TypeFacts {
    by_value_closures: HashMap<TypeId, HashSet<TypeId>>,
}

/// Which lifetimes a generated type mentions: `'r` (result storage, for
/// lists and references to recursive values), `'t` (tree) and `'s` (source).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct LifetimeUsage {
    pub(crate) rows: bool,
    pub(crate) tree: bool,
    pub(crate) source: bool,
}

impl LifetimeUsage {
    /// Generic parameter list, `'r` first, with `storage` naming `'r`.
    pub(crate) fn params(self, storage: &str) -> String {
        let mut params = Vec::new();
        if self.rows {
            params.push(storage);
        }
        if self.tree {
            params.push("'t");
        }
        if self.source {
            params.push("'s");
        }
        if params.is_empty() {
            return String::new();
        }
        format!("<{}>", params.join(", "))
    }
}

impl TypeFacts {
    pub(super) fn compute(types: &TypeAnalysis) -> Self {
        let reachable = collect_reachable(types);
        Self {
            by_value_closures: ref_target_closures(types, &reachable),
        }
    }

    pub(super) fn is_boxed_in(&self, item_ty: TypeId, ref_ty: TypeId) -> bool {
        self.by_value_closures
            .get(&ref_ty)
            .is_some_and(|closure| closure.contains(&item_ty))
    }
}

fn collect_reachable(types: &TypeAnalysis) -> Vec<TypeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack: Vec<TypeId> = types
        .iter_def_output()
        .filter_map(|(_, output)| output.value())
        .collect();

    while let Some(ty) = stack.pop() {
        if !seen.insert(ty) {
            continue;
        }
        out.push(ty);
        stack.extend(types.expect_type_shape(ty).child_type_ids());
        if let TypeShape::Ref(declaration) = types.expect_type_shape(ty)
            && let Some(target) = types.declaration_body(*declaration)
        {
            stack.push(target);
        }
    }

    out
}

fn ref_target_closures(
    types: &TypeAnalysis,
    reachable: &[TypeId],
) -> HashMap<TypeId, HashSet<TypeId>> {
    reachable
        .iter()
        .filter_map(|&ty| match types.expect_type_shape(ty) {
            TypeShape::Ref(declaration) => types
                .declaration_body(*declaration)
                .map(|target| (ty, by_value_closure(types, target))),
            _ => None,
        })
        .collect()
}

fn by_value_closure(types: &TypeAnalysis, from: TypeId) -> HashSet<TypeId> {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(ty) = stack.pop() {
        if !seen.insert(ty) {
            continue;
        }
        match types.expect_type_shape(ty) {
            TypeShape::List { .. } => {}
            TypeShape::Ref(declaration) => {
                if let Some(target) = types.declaration_body(*declaration) {
                    stack.push(target);
                }
            }
            shape => stack.extend(shape.child_type_ids()),
        }
    }
    seen
}
