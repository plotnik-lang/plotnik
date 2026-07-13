//! Unification logic for alternation alternatives.
//!
//! Handles merging `PatternFlow` from different alternatives.
//! Consumed labeled alternations don't unify — they produce variant types directly.

use std::collections::BTreeMap;

use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{
    ListMinimum, PatternFlow, RecordField, TYPE_NO_VALUE, TypeId, TypeShape,
};
use crate::core::Symbol;

/// Error during type unification.
#[derive(Clone, Debug)]
pub enum UnifyError {
    /// Capture has incompatible types across alternatives.
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
        return Ok(PatternFlow::NoValue);
    };

    iter.try_fold(first, |acc, flow| unify_flow_in(ctx, acc, flow))
}

/// Unify two `PatternFlow`s from alternation alternatives.
///
/// Rules:
/// - NoValue ∪ NoValue → NoValue
/// - NoValue ∪ Fields(s) → Fields(make_all_optional(s))
/// - Fields(a) ∪ Fields(b) → Fields(merge_fields(a, b))
/// - Value is an uncaptured pending value (a bare reference); it is dropped
///   like any uncaptured match, so it unifies as NoValue.
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
    let a = drop_pending_value(a);
    let b = drop_pending_value(b);

    match (a, b) {
        (PatternFlow::NoValue, PatternFlow::NoValue) => Ok(PatternFlow::NoValue),

        // NoValue ∪ Fields -> Fields (every field is absent in the no-value alternative)
        (PatternFlow::NoValue, PatternFlow::Fields(id))
        | (PatternFlow::Fields(id), PatternFlow::NoValue) => {
            let fields = ctx.in_progress().expect_record_fields(id).clone();
            let relaxed = relax_all_for_absence(ctx, fields);
            Ok(PatternFlow::Fields(ctx.intern_record(relaxed)))
        }

        (PatternFlow::Fields(a_id), PatternFlow::Fields(b_id)) => {
            let a_fields = ctx.in_progress().expect_record_fields(a_id).clone();
            let b_fields = ctx.in_progress().expect_record_fields(b_id).clone();

            let merged = merge_fields(ctx, a_fields, b_fields)?;
            Ok(PatternFlow::Fields(ctx.intern_record(merged)))
        }

        // `drop_pending_value` rewrites every Value to NoValue; the remaining
        // variants (NoValue, Fields) are matched exhaustively.
        _ => unreachable!("unify_flow: unexpected PatternFlow variant after dropping values"),
    }
}

fn drop_pending_value(flow: PatternFlow) -> PatternFlow {
    match flow {
        PatternFlow::Value(_) => PatternFlow::NoValue,
        other => other,
    }
}

/// Relax a field that is absent from some alternative, keeping the output shape stable
/// (every key present).
///
/// A list stays present as a (possibly empty) list when the list itself is the
/// field type: the absent alternative emits `[]`, never null, so it relaxes to
/// zero-or-more. Everything else becomes an option. In particular,
/// `Option<List<T>>` remains an option: `((x)+ @a)?` emits null when its `?` is
/// skipped, so forcing it to a non-null `[]` would make the declared type lie.
fn relax_for_absence(ctx: &mut TypeAnalysisBuilder, info: RecordField) -> RecordField {
    if let Some(TypeShape::List { element, .. }) = ctx.in_progress().type_shape(info.final_type) {
        let element = *element;
        let list = ctx.intern_type(TypeShape::List {
            element,
            minimum: ListMinimum::Zero,
        });
        return RecordField::new(list);
    }
    RecordField::new(ctx.intern_option(info.final_type))
}

/// Relax every field in a map for absence (see [`relax_for_absence`]).
fn relax_all_for_absence(
    ctx: &mut TypeAnalysisBuilder,
    fields: BTreeMap<Symbol, RecordField>,
) -> BTreeMap<Symbol, RecordField> {
    fields
        .into_iter()
        .map(|(k, v)| (k, relax_for_absence(ctx, v)))
        .collect()
}

/// Merge two field maps.
///
/// Rules:
/// - Keys in both: types must be compatible.
/// - Keys in only one: relaxed for absence (an option, or an empty-able list).
fn merge_fields(
    ctx: &mut TypeAnalysisBuilder,
    a: BTreeMap<Symbol, RecordField>,
    mut b: BTreeMap<Symbol, RecordField>,
) -> Result<BTreeMap<Symbol, RecordField>, UnifyError> {
    let mut result = BTreeMap::new();
    let mut absent_fields = Vec::new();

    for (key, a_info) in a {
        if let Some(b_info) = b.remove(&key) {
            let final_type = unify_type_ids(ctx, a_info.final_type, b_info.final_type, key)?;
            result.insert(key, RecordField::new(final_type));
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
/// Records and variant types mint a fresh id per occurrence (nominal typing), so two
/// alternatives capturing structurally identical anonymous composites carry
/// different ids for the same shape — compare structurally, keeping the first
/// alternative's id. `NoValue` is the identity element (compatible with any type).
fn unify_type_ids(
    ctx: &mut TypeAnalysisBuilder,
    a: TypeId,
    b: TypeId,
    field: Symbol,
) -> Result<TypeId, UnifyError> {
    if a == TYPE_NO_VALUE {
        return Ok(b);
    }
    if b == TYPE_NO_VALUE {
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

    match (&a_shape, &b_shape) {
        (TypeShape::Option(a_inner), TypeShape::Option(b_inner)) => {
            let inner = unify_type_ids(ctx, *a_inner, *b_inner, field)?;
            return Ok(ctx.intern_option(inner));
        }
        (TypeShape::Option(inner), _) => {
            let inner = unify_type_ids(ctx, *inner, b, field)?;
            return Ok(ctx.intern_option(inner));
        }
        (_, TypeShape::Option(inner)) => {
            let inner = unify_type_ids(ctx, a, *inner, field)?;
            return Ok(ctx.intern_option(inner));
        }
        _ => {}
    }

    // Lists that differ only in minimum length relax to zero-or-more: only one
    // alternative matches, so the merged list is non-empty only when the `+`
    // alternative did — `T[]+ ∪ T[]* = T[]*`.
    if let (
        TypeShape::List {
            element: ea,
            minimum: ma,
        },
        TypeShape::List {
            element: eb,
            minimum: mb,
        },
    ) = (&a_shape, &b_shape)
        && ma != mb
        && ctx.types_structurally_equal(*ea, *eb)
    {
        return Ok(if *ma == ListMinimum::One { b } else { a });
    }

    Err(UnifyError::IncompatibleTypes { field })
}
