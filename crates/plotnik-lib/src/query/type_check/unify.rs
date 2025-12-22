//! Unification logic for alternation branches.
//!
//! Handles merging TypeFlow from different branches of untagged alternations.
//! Tagged alternations don't unify—they produce Enum types directly.

use std::collections::BTreeMap;

use super::context::TypeContext;
use super::symbol::Symbol;
use super::types::{FieldInfo, TYPE_NODE, TYPE_VOID, TypeFlow, TypeId};

/// Error during type unification.
#[derive(Clone, Debug)]
pub enum UnifyError {
    /// Scalar type appeared in untagged alternation (needs tagging)
    ScalarInUntagged,
    /// Capture has incompatible types across branches
    IncompatibleTypes { field: Symbol },
    /// Capture has incompatible struct shapes across branches
    IncompatibleStructs { field: Symbol },
    /// Array element types don't match
    IncompatibleArrayElements { field: Symbol },
}

impl UnifyError {
    pub fn field_symbol(&self) -> Option<Symbol> {
        match self {
            UnifyError::ScalarInUntagged => None,
            UnifyError::IncompatibleTypes { field }
            | UnifyError::IncompatibleStructs { field }
            | UnifyError::IncompatibleArrayElements { field } => Some(*field),
        }
    }
}

/// Unify two TypeFlows from alternation branches.
///
/// Rules:
/// - Void ∪ Void → Void
/// - Void ∪ Bubble(s) → Bubble(make_all_optional(s))
/// - Bubble(a) ∪ Bubble(b) → Bubble(merge_fields(a, b))
/// - Scalar in untagged → Error (use tagged alternation instead)
pub fn unify_flow(ctx: &mut TypeContext, a: TypeFlow, b: TypeFlow) -> Result<TypeFlow, UnifyError> {
    match (a, b) {
        (TypeFlow::Void, TypeFlow::Void) => Ok(TypeFlow::Void),

        (TypeFlow::Void, TypeFlow::Bubble(id)) | (TypeFlow::Bubble(id), TypeFlow::Void) => {
            let fields = ctx.get_struct_fields(id).cloned().unwrap_or_default();
            let optional_fields = make_all_optional(fields);
            Ok(TypeFlow::Bubble(ctx.intern_struct(optional_fields)))
        }

        (TypeFlow::Bubble(a_id), TypeFlow::Bubble(b_id)) => {
            let a_fields = ctx.get_struct_fields(a_id).cloned().unwrap_or_default();
            let b_fields = ctx.get_struct_fields(b_id).cloned().unwrap_or_default();
            let merged = merge_fields(a_fields, b_fields)?;
            Ok(TypeFlow::Bubble(ctx.intern_struct(merged)))
        }

        // Scalars can't appear in untagged alternations
        (TypeFlow::Scalar(_), _) | (_, TypeFlow::Scalar(_)) => Err(UnifyError::ScalarInUntagged),
    }
}

/// Unify multiple flows from alternation branches.
pub fn unify_flows(
    ctx: &mut TypeContext,
    flows: impl IntoIterator<Item = TypeFlow>,
) -> Result<TypeFlow, UnifyError> {
    let mut iter = flows.into_iter();
    let Some(first) = iter.next() else {
        return Ok(TypeFlow::Void);
    };

    iter.try_fold(first, |acc, flow| unify_flow(ctx, acc, flow))
}

/// Make all fields in a map optional.
fn make_all_optional(fields: BTreeMap<Symbol, FieldInfo>) -> BTreeMap<Symbol, FieldInfo> {
    fields
        .into_iter()
        .map(|(k, v)| (k, v.make_optional()))
        .collect()
}

/// Merge two field maps.
///
/// Rules:
/// - Keys in both: types must be compatible, field is required iff required in both
/// - Keys in only one: field becomes optional
fn merge_fields(
    a: BTreeMap<Symbol, FieldInfo>,
    b: BTreeMap<Symbol, FieldInfo>,
) -> Result<BTreeMap<Symbol, FieldInfo>, UnifyError> {
    let mut result = BTreeMap::new();

    // Process all keys from a
    for (key, a_info) in &a {
        if let Some(b_info) = b.get(key) {
            // Key exists in both: unify types
            let unified_type = unify_type_ids(a_info.type_id, b_info.type_id, *key)?;
            let optional = a_info.optional || b_info.optional;
            result.insert(
                *key,
                FieldInfo {
                    type_id: unified_type,
                    optional,
                },
            );
        } else {
            // Key only in a: make optional
            result.insert(*key, a_info.make_optional());
        }
    }

    // Process keys only in b
    for (key, b_info) in b {
        if !a.contains_key(&key) {
            result.insert(key, b_info.make_optional());
        }
    }

    Ok(result)
}

/// Unify two type IDs.
///
/// For now, types must match exactly (except Node is compatible with Node).
/// Future: could allow structural subtyping for structs.
fn unify_type_ids(a: TypeId, b: TypeId, field: Symbol) -> Result<TypeId, UnifyError> {
    if a == b {
        return Ok(a);
    }

    // Both are Node type - compatible
    if a == TYPE_NODE && b == TYPE_NODE {
        return Ok(TYPE_NODE);
    }

    // Void is compatible with anything (treat as identity)
    if a == TYPE_VOID {
        return Ok(b);
    }
    if b == TYPE_VOID {
        return Ok(a);
    }

    // Type mismatch
    Err(UnifyError::IncompatibleTypes { field })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unify_void_void() {
        let mut ctx = TypeContext::new();
        let result = unify_flow(&mut ctx, TypeFlow::Void, TypeFlow::Void);
        assert!(matches!(result, Ok(TypeFlow::Void)));
    }

    #[test]
    fn unify_void_bubble() {
        let mut ctx = TypeContext::new();
        let x = ctx.intern("x");
        let struct_id = ctx.intern_single_field(x, FieldInfo::required(TYPE_NODE));

        let result = unify_flow(&mut ctx, TypeFlow::Void, TypeFlow::Bubble(struct_id)).unwrap();

        match result {
            TypeFlow::Bubble(id) => {
                let fields = ctx.get_struct_fields(id).unwrap();
                assert!(fields.get(&x).unwrap().optional);
            }
            _ => panic!("expected Bubble"),
        }
    }

    #[test]
    fn unify_bubble_merge() {
        let mut ctx = TypeContext::new();
        let x = ctx.intern("x");
        let y = ctx.intern("y");

        let a_id = ctx.intern_single_field(x, FieldInfo::required(TYPE_NODE));

        let mut b_fields = BTreeMap::new();
        b_fields.insert(x, FieldInfo::required(TYPE_NODE));
        b_fields.insert(y, FieldInfo::required(TYPE_NODE));
        let b_id = ctx.intern_struct(b_fields);

        let result = unify_flow(&mut ctx, TypeFlow::Bubble(a_id), TypeFlow::Bubble(b_id)).unwrap();

        match result {
            TypeFlow::Bubble(id) => {
                let fields = ctx.get_struct_fields(id).unwrap();
                // x is in both, so required
                assert!(!fields.get(&x).unwrap().optional);
                // y only in b, so optional
                assert!(fields.get(&y).unwrap().optional);
            }
            _ => panic!("expected Bubble"),
        }
    }

    #[test]
    fn unify_scalar_error() {
        let mut ctx = TypeContext::new();
        let result = unify_flow(&mut ctx, TypeFlow::Scalar(TYPE_NODE), TypeFlow::Void);
        assert!(matches!(result, Err(UnifyError::ScalarInUntagged)));
    }
}
