//! Unification logic for alternation branches.
//!
//! Handles merging PatternFlow from different branches of union alternations.
//! Consumed enum alternations don't unify — they produce Enum types directly.

use std::collections::BTreeMap;

use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{
    FieldInfo, PatternFlow, TYPE_VOID, TypeId, TypeShape,
};
use crate::core::Symbol;

/// Error during type unification.
#[derive(Clone, Debug)]
pub enum UnifyError {
    /// Capture has incompatible types across branches
    IncompatibleTypes { field: Symbol },
}

impl UnifyError {
    pub fn field(&self) -> Symbol {
        match self {
            Self::IncompatibleTypes { field } => *field,
        }
    }
}

pub fn unify_flows(
    ctx: &mut TypeAnalysisBuilder,
    flows: impl IntoIterator<Item = PatternFlow>,
) -> Result<PatternFlow, UnifyError> {
    let mut iter = flows.into_iter();
    let Some(first) = iter.next() else {
        return Ok(PatternFlow::Void);
    };

    iter.try_fold(first, |acc, flow| unify_flow_in(ctx, acc, flow))
}

/// Unify two PatternFlows from alternation branches.
///
/// Rules:
/// - Void ∪ Void → Void
/// - Void ∪ Fields(s) → Fields(make_all_optional(s))
/// - Fields(a) ∪ Fields(b) → Fields(merge_fields(a, b))
/// - Value is an uncaptured pending value (a bare reference); it is suppressed
///   like any uncaptured match, so it unifies as Void.
#[cfg(test)]
pub fn unify_flow(
    ctx: &mut TypeAnalysisBuilder,
    a: PatternFlow,
    b: PatternFlow,
) -> Result<PatternFlow, UnifyError> {
    unify_flow_in(ctx, a, b)
}

fn unify_flow_in(
    ctx: &mut TypeAnalysisBuilder,
    a: PatternFlow,
    b: PatternFlow,
) -> Result<PatternFlow, UnifyError> {
    let a = suppress_value(a);
    let b = suppress_value(b);

    match (a, b) {
        (PatternFlow::Void, PatternFlow::Void) => Ok(PatternFlow::Void),

        // Void ∪ Fields -> Fields (every field is absent in the Void branch)
        (PatternFlow::Void, PatternFlow::Fields(id))
        | (PatternFlow::Fields(id), PatternFlow::Void) => {
            let fields = ctx.in_progress().expect_struct_fields(id).clone();
            let relaxed = relax_all_for_absence(ctx, fields);
            Ok(PatternFlow::Fields(ctx.intern_struct(relaxed)))
        }

        (PatternFlow::Fields(a_id), PatternFlow::Fields(b_id)) => {
            let a_fields = ctx.in_progress().expect_struct_fields(a_id).clone();
            let b_fields = ctx.in_progress().expect_struct_fields(b_id).clone();

            let merged = merge_fields(ctx, a_fields, b_fields)?;
            Ok(PatternFlow::Fields(ctx.intern_struct(merged)))
        }

        // `suppress_value` above rewrites every Value to Void; the remaining
        // variants (Void, Fields) are matched exhaustively.
        _ => unreachable!("unify_flow: unexpected PatternFlow variant after value suppression"),
    }
}

fn suppress_value(flow: PatternFlow) -> PatternFlow {
    match flow {
        PatternFlow::Value(_) => PatternFlow::Void,
        other => other,
    }
}

/// Relax a field that is absent from some branch, keeping the output shape stable
/// (every key present).
///
/// A *required* list stays present as a (possibly empty) array — the absent branch
/// emits `[]`, never null — so it relaxes to zero-or-more. Everything else becomes
/// nullable, including an already-optional list: `((x)+ @a)?` emits null when its
/// `?` is skipped, so forcing it to a non-null `[]` here would make the declared
/// type lie. Nullability (`optional`), not the array shape, decides the default,
/// which keeps inference in lockstep with what the emitter writes.
fn relax_for_absence(ctx: &mut TypeAnalysisBuilder, info: FieldInfo) -> FieldInfo {
    if !info.optional
        && let Some(TypeShape::Array { element, .. }) = ctx.in_progress().type_shape(info.type_id)
    {
        let element = *element;
        let array = ctx.intern_type(TypeShape::Array {
            element,
            non_empty: false,
        });
        return FieldInfo::required(array);
    }
    info.make_optional()
}

/// Relax every field in a map for absence (see [`relax_for_absence`]).
fn relax_all_for_absence(
    ctx: &mut TypeAnalysisBuilder,
    fields: BTreeMap<Symbol, FieldInfo>,
) -> BTreeMap<Symbol, FieldInfo> {
    fields
        .into_iter()
        .map(|(k, v)| (k, relax_for_absence(ctx, v)))
        .collect()
}

/// Merge two field maps.
///
/// Rules:
/// - Keys in both: types must be compatible, field is required iff required in both.
/// - Keys in only one: relaxed for absence (nullable, or an empty-able list).
fn merge_fields(
    ctx: &mut TypeAnalysisBuilder,
    a: BTreeMap<Symbol, FieldInfo>,
    mut b: BTreeMap<Symbol, FieldInfo>,
) -> Result<BTreeMap<Symbol, FieldInfo>, UnifyError> {
    let mut result = BTreeMap::new();
    let mut absent_fields = Vec::new();

    for (key, a_info) in a {
        if let Some(b_info) = b.remove(&key) {
            let type_id = unify_type_ids(ctx, a_info.type_id, b_info.type_id, key)?;
            let optional = a_info.optional || b_info.optional;
            result.insert(key, FieldInfo::with_optional(type_id, optional));
        } else {
            absent_fields.push((key, a_info));
        }
    }

    absent_fields.extend(b);
    for (key, info) in absent_fields {
        result.insert(key, relax_for_absence(ctx, info));
    }

    Ok(result)
}

/// Unify two type IDs.
///
/// Structs and enums mint a fresh id per occurrence (nominal typing), so two
/// branches capturing structurally identical anonymous composites carry
/// different ids for the same shape — compare structurally, keeping the first
/// branch's id. `Void` is the identity element (compatible with any type).
fn unify_type_ids(
    ctx: &mut TypeAnalysisBuilder,
    a: TypeId,
    b: TypeId,
    field: Symbol,
) -> Result<TypeId, UnifyError> {
    if a == TYPE_VOID {
        return Ok(b);
    }
    if b == TYPE_VOID {
        return Ok(a);
    }

    if ctx.types_structurally_equal(a, b) {
        return Ok(a);
    }

    let a_shape = ctx
        .in_progress()
        .type_shape(a)
        .cloned()
        .expect("unified field type is registered");
    let b_shape = ctx
        .in_progress()
        .type_shape(b)
        .cloned()
        .expect("unified field type is registered");

    // Arrays that differ only in cardinality relax to zero-or-more: only one
    // branch matches, so the merged list is non-empty only when the `+` branch
    // did — `T[]+ ∪ T[]* = T[]*`.
    if let (
        TypeShape::Array {
            element: ea,
            non_empty: na,
        },
        TypeShape::Array {
            element: eb,
            non_empty: nb,
        },
    ) = (&a_shape, &b_shape)
        && na != nb
        && ctx.types_structurally_equal(*ea, *eb)
    {
        return Ok(if *na { b } else { a });
    }

    Err(UnifyError::IncompatibleTypes { field })
}
