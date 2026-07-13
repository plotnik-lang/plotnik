//! Per-type facts the renderer needs: lifetime usage and `Box` cut points.
//!
//! Both are properties of the type *graph*, which only closes cycles through
//! `Ref` shapes (structs and enums are fresh, nominal, and built as finite
//! trees per definition; only a reference re-enters another definition). So:
//!
//! - `needs_lifetime` is a least fixpoint: a type carries `'t` iff it can
//!   reach a `Node` leaf. A pure cycle contributes nothing, which is why the
//!   iteration starts at `false` everywhere.
//! - a `Ref` occurrence is boxed iff it closes a by-value cycle through the
//!   item declaration it is rendered in: its target reaches that *item* back
//!   through edges that stay on the stack. `Vec` already heap-indirects, so
//!   array elements are not by-value edges (and a ref under an array is
//!   never boxed); `Option` stores inline, so it is one. Keying on the
//!   enclosing item rather than the ref node keeps an off-cycle use of a
//!   recursive type unboxed (`Q { expr: Expr }`) while every declaration a
//!   cycle actually passes through cuts it (`Paren { inner: Box<Expr> }`) —
//!   any surviving by-value cycle would have to pass through some item whose
//!   rendering, by this rule, boxed it.

use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};

pub(super) struct TypeFacts {
    lifetimes: HashMap<TypeId, LifetimeUsage>,
    /// Per `Ref` node: everything its target reaches through by-value
    /// containment, the target itself included. A ref rendered inside item
    /// `I` closes a by-value cycle exactly when `I` is in its target's
    /// closure — the membership [`Self::is_boxed_in`] tests.
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

    /// Whether this `Ref` occurrence, rendered by value inside `item_ty`'s
    /// declaration, renders as `Box<...>`. Occurrences under an array never
    /// ask — `Vec` already indirects, so no cycle through them is by-value.
    pub(super) fn is_boxed_in(&self, item_ty: TypeId, ref_ty: TypeId) -> bool {
        self.by_value_closures
            .get(&ref_ty)
            .is_some_and(|closure| closure.contains(&item_ty))
    }
}

/// Every type id reachable from a definition output, following refs into
/// their targets. This is exactly the set the renderer can ask facts about.
fn collect_reachable(types: &TypeAnalysis) -> Vec<TypeId> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut stack: Vec<TypeId> = types
        .iter_def_output()
        .filter(|&(_, ty)| ty != TYPE_VOID)
        .map(|(_, ty)| ty)
        .collect();

    while let Some(ty) = stack.pop() {
        if !seen.insert(ty) {
            continue;
        }
        out.push(ty);
        stack.extend(types.expect_type_shape(ty).child_type_ids());
        if let TypeShape::Ref(def_id) = types.expect_type_shape(ty) {
            let target = types.expect_def_output(*def_id);
            if target != TYPE_VOID {
                stack.push(target);
            }
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
                TypeShape::Node | TypeShape::Custom(_) => LifetimeUsage {
                    tree: true,
                    source: false,
                },
                TypeShape::Text => LifetimeUsage {
                    tree: false,
                    source: true,
                },
                TypeShape::Ref(def_id) => {
                    let target = types.expect_def_output(*def_id);
                    if target == TYPE_VOID {
                        LifetimeUsage {
                            tree: true,
                            source: false,
                        }
                    } else {
                        facts[&target]
                    }
                }
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
            TypeShape::Ref(def_id) => {
                let target = types.expect_def_output(*def_id);
                (target != TYPE_VOID).then(|| (ty, by_value_closure(types, target)))
            }
            _ => None,
        })
        .collect()
}

/// Everything reachable from `from` through by-value containment, `from`
/// included: the declarations a by-value path out of `from` can pass through
/// before any heap indirection.
fn by_value_closure(types: &TypeAnalysis, from: TypeId) -> HashSet<TypeId> {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(ty) = stack.pop() {
        if !seen.insert(ty) {
            continue;
        }
        match types.expect_type_shape(ty) {
            // An array stores its elements on the heap: not a by-value edge.
            TypeShape::Array { .. } => {}
            TypeShape::Ref(def_id) => {
                let target = types.expect_def_output(*def_id);
                if target != TYPE_VOID {
                    stack.push(target);
                }
            }
            shape => stack.extend(shape.child_type_ids()),
        }
    }
    seen
}
