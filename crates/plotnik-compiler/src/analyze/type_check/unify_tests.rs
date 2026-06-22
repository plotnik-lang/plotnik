use std::collections::BTreeMap;

use super::*;
use crate::analyze::type_check::TYPE_NODE;
use plotnik_core::Interner;

#[test]
fn unify_void_void() {
    let mut ctx = TypeContext::new();
    let result = unify_flow(&mut ctx, OutputFlow::Void, OutputFlow::Void);
    assert!(matches!(result, Ok(OutputFlow::Void)));
}

#[test]
fn unify_void_bubble() {
    let mut ctx = TypeContext::new();
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let struct_id = ctx.intern_single_field(x, FieldInfo::required(TYPE_NODE));

    let result = unify_flow(&mut ctx, OutputFlow::Void, OutputFlow::Fields(struct_id)).unwrap();

    match result {
        OutputFlow::Fields(id) => {
            let fields = ctx.struct_fields(id).unwrap();
            assert!(fields.get(&x).unwrap().optional);
        }
        _ => panic!("expected Fields"),
    }
}

#[test]
fn unify_bubble_merge() {
    let mut ctx = TypeContext::new();
    let mut interner = Interner::new();
    let x = interner.intern("x");
    let y = interner.intern("y");

    let a_id = ctx.intern_single_field(x, FieldInfo::required(TYPE_NODE));

    let mut b_fields = BTreeMap::new();
    b_fields.insert(x, FieldInfo::required(TYPE_NODE));
    b_fields.insert(y, FieldInfo::required(TYPE_NODE));
    let b_id = ctx.intern_struct(b_fields);

    let result = unify_flow(&mut ctx, OutputFlow::Fields(a_id), OutputFlow::Fields(b_id)).unwrap();

    match result {
        OutputFlow::Fields(id) => {
            let fields = ctx.struct_fields(id).unwrap();
            // x is in both, so required
            assert!(!fields.get(&x).unwrap().optional);
            // y only in b, so optional
            assert!(fields.get(&y).unwrap().optional);
        }
        _ => panic!("expected Fields"),
    }
}

#[test]
fn unify_scalar_error() {
    let mut ctx = TypeContext::new();
    let result = unify_flow(&mut ctx, OutputFlow::Value(TYPE_NODE), OutputFlow::Void);
    assert!(matches!(result, Err(UnifyError::ScalarInUnion)));
}
