//! Unification logic for alternation branches.
//!
//! Handles merging OutputFlow from different branches of union alternations.
//! Enum alternations don't unify — they produce Enum types directly.

use std::collections::BTreeMap;

use super::context::TypeAnalysisBuilder;
use super::def_id::Symbol;
use super::types::{FieldInfo, OutputFlow, TYPE_VOID, TypeId, TypeShape};

/// Error during type unification.
#[derive(Clone, Debug)]
pub enum UnifyError {
    /// Scalar type appeared in union alternation (needs a label)
    ScalarInUnion,
    /// Capture has incompatible types across branches
    IncompatibleTypes { field: Symbol },
}

pub fn unify_flows(
    ctx: &mut TypeAnalysisBuilder,
    flows: impl IntoIterator<Item = OutputFlow>,
) -> Result<OutputFlow, UnifyError> {
    let mut iter = flows.into_iter();
    let Some(first) = iter.next() else {
        return Ok(OutputFlow::Void);
    };

    iter.try_fold(first, |acc, flow| unify_flow(ctx, acc, flow))
}

/// Unify two OutputFlows from alternation branches.
///
/// Rules:
/// - Void ∪ Void → Void
/// - Void ∪ Fields(s) → Fields(make_all_optional(s))
/// - Fields(a) ∪ Fields(b) → Fields(merge_fields(a, b))
/// - Value in union → Error
pub fn unify_flow(
    ctx: &mut TypeAnalysisBuilder,
    a: OutputFlow,
    b: OutputFlow,
) -> Result<OutputFlow, UnifyError> {
    // Union alternations cannot contain scalars.
    if matches!(a, OutputFlow::Value(_)) || matches!(b, OutputFlow::Value(_)) {
        return Err(UnifyError::ScalarInUnion);
    }

    match (a, b) {
        (OutputFlow::Void, OutputFlow::Void) => Ok(OutputFlow::Void),

        // Void ∪ Fields -> Fields (every field is absent in the Void branch)
        (OutputFlow::Void, OutputFlow::Fields(id)) | (OutputFlow::Fields(id), OutputFlow::Void) => {
            let fields = ctx.expect_struct_fields(id).clone();
            let relaxed = relax_all_for_absence(ctx, fields);
            Ok(OutputFlow::Fields(ctx.intern_struct(relaxed)))
        }

        (OutputFlow::Fields(a_id), OutputFlow::Fields(b_id)) => {
            let a_fields = ctx.expect_struct_fields(a_id).clone();
            let b_fields = ctx.expect_struct_fields(b_id).clone();

            let merged = merge_fields(ctx, a_fields, b_fields)?;
            Ok(OutputFlow::Fields(ctx.intern_struct(merged)))
        }

        // The scalar guard above (`matches!(a|b, Value)`) already returns Err.
        // Every remaining OutputFlow variant (Void, Fields) is matched explicitly above.
        _ => unreachable!("unify_flow: unexpected OutputFlow variant after scalar guard"),
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
        && let Some(TypeShape::Array { element, .. }) = ctx.type_shape(info.type_id)
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

    for (key, a_info) in a {
        if let Some(b_info) = b.remove(&key) {
            let type_id = unify_type_ids(a_info.type_id, b_info.type_id, key)?;
            let optional = a_info.optional || b_info.optional;
            result.insert(key, FieldInfo { type_id, optional });
        } else {
            result.insert(key, relax_for_absence(ctx, a_info));
        }
    }

    for (key, b_info) in b {
        result.insert(key, relax_for_absence(ctx, b_info));
    }

    Ok(result)
}

/// Unify two type IDs.
///
/// Types must match exactly; `Void` is the identity element (compatible with any type).
fn unify_type_ids(a: TypeId, b: TypeId, field: Symbol) -> Result<TypeId, UnifyError> {
    if a == b {
        return Ok(a);
    }

    // Void is compatible with anything (treat as identity)
    if a == TYPE_VOID {
        return Ok(b);
    }
    if b == TYPE_VOID {
        return Ok(a);
    }

    Err(UnifyError::IncompatibleTypes { field })
}
