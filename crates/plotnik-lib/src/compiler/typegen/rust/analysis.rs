//! Per-type facts the renderer needs: lifetime usage and `Box` cut points.
//!
//! Both are properties of the type *graph*, which only closes cycles through
//! `Ref` shapes (structs and enums are fresh, nominal, and built as finite
//! trees per definition; only a reference re-enters another definition). So:
//!
//! - `needs_lifetime` is a least fixpoint: a type carries `'t` iff it can
//!   reach a `Node` leaf. A pure cycle contributes nothing, which is why the
//!   iteration starts at `false` everywhere.
//! - a `Ref` occurrence is boxed iff it participates in a by-value cycle —
//!   its target reaches back to it through edges that stay on the stack.
//!   `Vec` already heap-indirects, so array elements are not by-value edges;
//!   `Option` stores inline, so it is one. Boxing every on-cycle ref edge
//!   (rather than a minimal cut) keeps the decision local and stable under
//!   definition reordering.

use std::collections::{HashMap, HashSet};

use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeId, TypeShape};

pub(super) struct TypeFacts {
    needs_lifetime: HashMap<TypeId, bool>,
    boxed_refs: HashSet<TypeId>,
}

impl TypeFacts {
    pub(super) fn compute(types: &TypeAnalysis) -> Self {
        let reachable = collect_reachable(types);
        Self {
            needs_lifetime: lifetime_fixpoint(types, &reachable),
            boxed_refs: on_cycle_refs(types, &reachable),
        }
    }

    /// Whether the type's rendering mentions `'t` (transitively holds a node).
    pub(super) fn needs_lifetime(&self, ty: TypeId) -> bool {
        *self
            .needs_lifetime
            .get(&ty)
            .expect("lifetime facts cover every type reachable from a definition output")
    }

    /// Whether this `Ref` occurrence renders as `Box<...>`.
    pub(super) fn is_boxed(&self, ref_ty: TypeId) -> bool {
        self.boxed_refs.contains(&ref_ty)
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

fn lifetime_fixpoint(types: &TypeAnalysis, reachable: &[TypeId]) -> HashMap<TypeId, bool> {
    let mut facts: HashMap<TypeId, bool> = reachable.iter().map(|&ty| (ty, false)).collect();

    loop {
        let mut changed = false;
        for &ty in reachable {
            if facts[&ty] {
                continue;
            }
            let holds_node = match types.expect_type_shape(ty) {
                // Custom is a named alias of Node; a void-targeted Ref
                // renders as Node too.
                TypeShape::Node | TypeShape::Custom(_) => true,
                TypeShape::Ref(def_id) => {
                    let target = types.expect_def_output(*def_id);
                    target == TYPE_VOID || facts[&target]
                }
                shape => shape.child_type_ids().any(|child| facts[&child]),
            };
            if holds_node {
                facts.insert(ty, true);
                changed = true;
            }
        }
        if !changed {
            return facts;
        }
    }
}

fn on_cycle_refs(types: &TypeAnalysis, reachable: &[TypeId]) -> HashSet<TypeId> {
    reachable
        .iter()
        .filter(|&&ty| match types.expect_type_shape(ty) {
            TypeShape::Ref(def_id) => {
                let target = types.expect_def_output(*def_id);
                target != TYPE_VOID && reaches_by_value(types, target, ty)
            }
            _ => false,
        })
        .copied()
        .collect()
}

/// Whether `to` is reachable from `from` through by-value containment.
fn reaches_by_value(types: &TypeAnalysis, from: TypeId, to: TypeId) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(ty) = stack.pop() {
        if ty == to {
            return true;
        }
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
    false
}
