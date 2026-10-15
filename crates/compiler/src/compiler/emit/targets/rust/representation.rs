use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{TypeId, TypeShape};

pub(super) struct TypeFacts {
    lifetimes: HashMap<TypeId, LifetimeUsage>,
    by_value_closures: HashMap<TypeId, HashSet<TypeId>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct LifetimeUsage {
    pub(super) tree: bool,
    pub(super) source: bool,
}

impl LifetimeUsage {
    fn merge(&mut self, other: Self) {
        self.tree |= other.tree;
        self.source |= other.source;
    }
}

impl TypeFacts {
    pub(super) fn compute(types: &TypeAnalysis) -> Self {
        let reachable = collect_reachable(types);
        Self {
            lifetimes: lifetime_fixpoint(types, &reachable),
            by_value_closures: ref_target_closures(types, &reachable),
        }
    }

    pub(super) fn lifetime_usage(&self, ty: TypeId) -> LifetimeUsage {
        *self
            .lifetimes
            .get(&ty)
            .expect("lifetime facts cover every type reachable from a definition output")
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

fn lifetime_fixpoint(types: &TypeAnalysis, reachable: &[TypeId]) -> HashMap<TypeId, LifetimeUsage> {
    let mut facts: HashMap<TypeId, LifetimeUsage> = reachable
        .iter()
        .map(|&ty| (ty, LifetimeUsage::default()))
        .collect();

    loop {
        let mut changed = false;
        for &ty in reachable {
            let mut usage = match types.expect_type_shape(ty) {
                TypeShape::Node => LifetimeUsage {
                    tree: true,
                    source: false,
                },
                TypeShape::Text => LifetimeUsage {
                    tree: false,
                    source: true,
                },
                TypeShape::Ref(declaration) => match types.declaration_body(*declaration) {
                    None => LifetimeUsage {
                        tree: true,
                        source: false,
                    },
                    Some(target) => facts[&target],
                },
                _ => LifetimeUsage::default(),
            };
            for child in types.expect_type_shape(ty).child_type_ids() {
                usage.merge(facts[&child]);
            }
            if usage != facts[&ty] {
                facts.insert(ty, usage);
                changed = true;
            }
        }
        if !changed {
            return facts;
        }
    }
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
