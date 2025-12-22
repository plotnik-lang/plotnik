//! Unification logic for alternation branches.
//!
//! Handles merging TypeFlow from different branches of untagged alternations.
//! Tagged alternations don't unify—they produce Enum types directly.

use std::collections::BTreeMap;

use super::symbol::Symbol;
use super::types::{FieldInfo, TYPE_NODE, TypeFlow, TypeId};

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
/// - Void ∪ Fields(f) → Fields(make_all_optional(f))
/// - Fields(a) ∪ Fields(b) → Fields(merge_fields(a, b))
/// - Scalar in untagged → Error (use tagged alternation instead)
pub fn unify_flow(a: TypeFlow, b: TypeFlow) -> Result<TypeFlow, UnifyError> {
    match (a, b) {
        (TypeFlow::Void, TypeFlow::Void) => Ok(TypeFlow::Void),

        (TypeFlow::Void, TypeFlow::Fields(f)) | (TypeFlow::Fields(f), TypeFlow::Void) => {
            Ok(TypeFlow::Fields(make_all_optional(f)))
        }

        (TypeFlow::Fields(a), TypeFlow::Fields(b)) => Ok(TypeFlow::Fields(merge_fields(a, b)?)),

        // Scalars can't appear in untagged alternations
        (TypeFlow::Scalar(_), _) | (_, TypeFlow::Scalar(_)) => Err(UnifyError::ScalarInUntagged),
    }
}

/// Unify multiple flows from alternation branches.
pub fn unify_flows(flows: impl IntoIterator<Item = TypeFlow>) -> Result<TypeFlow, UnifyError> {
    let mut iter = flows.into_iter();
    let Some(first) = iter.next() else {
        return Ok(TypeFlow::Void);
    };

    iter.try_fold(first, unify_flow)
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

    // Type mismatch
    Err(UnifyError::IncompatibleTypes { field })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_symbol(n: u32) -> Symbol {
        // For tests, create symbols directly. In real code, use Interner.
        // This is safe because tests don't need actual string resolution.
        unsafe { std::mem::transmute(n) }
    }

    #[test]
    fn unify_void_void() {
        let result = unify_flow(TypeFlow::Void, TypeFlow::Void);
        assert!(matches!(result, Ok(TypeFlow::Void)));
    }

    #[test]
    fn unify_void_fields() {
        let x = make_symbol(0);
        let mut fields = BTreeMap::new();
        fields.insert(x, FieldInfo::required(TYPE_NODE));

        let result = unify_flow(TypeFlow::Void, TypeFlow::Fields(fields)).unwrap();

        match result {
            TypeFlow::Fields(f) => {
                assert!(f.get(&x).unwrap().optional);
            }
            _ => panic!("expected Fields"),
        }
    }

    #[test]
    fn unify_fields_merge() {
        let x = make_symbol(0);
        let y = make_symbol(1);

        let mut a = BTreeMap::new();
        a.insert(x, FieldInfo::required(TYPE_NODE));

        let mut b = BTreeMap::new();
        b.insert(x, FieldInfo::required(TYPE_NODE));
        b.insert(y, FieldInfo::required(TYPE_NODE));

        let result = unify_flow(TypeFlow::Fields(a), TypeFlow::Fields(b)).unwrap();

        match result {
            TypeFlow::Fields(f) => {
                // x is in both, so required
                assert!(!f.get(&x).unwrap().optional);
                // y only in b, so optional
                assert!(f.get(&y).unwrap().optional);
            }
            _ => panic!("expected Fields"),
        }
    }

    #[test]
    fn unify_scalar_error() {
        let result = unify_flow(TypeFlow::Scalar(TYPE_NODE), TypeFlow::Void);
        assert!(matches!(result, Err(UnifyError::ScalarInUntagged)));
    }
}
