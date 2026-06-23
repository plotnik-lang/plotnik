use std::collections::BTreeMap;

use super::*;

#[test]
fn builtin_types_have_correct_ids() {
    let ctx = TypeAnalysisBuilder::new();

    assert_eq!(ctx.type_shape(TYPE_VOID), Some(&TypeShape::Void));
    assert_eq!(ctx.type_shape(TYPE_NODE), Some(&TypeShape::Node));
}

#[test]
fn type_interning_deduplicates() {
    let mut ctx = TypeAnalysisBuilder::new();

    let id1 = ctx.intern_type(TypeShape::Node);
    let id2 = ctx.intern_type(TypeShape::Node);

    assert_eq!(id1, id2);
    assert_eq!(id1, TYPE_NODE);
}

#[test]
fn struct_types_intern_correctly() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();

    let x_sym = interner.intern("x");
    let mut fields = BTreeMap::new();
    fields.insert(x_sym, FieldInfo::required(TYPE_NODE));

    let id1 = ctx.intern_type(TypeShape::Struct(fields.clone()));
    let id2 = ctx.intern_type(TypeShape::Struct(fields));

    assert_eq!(id1, id2);
}

#[test]
fn symbol_interning_works() {
    let mut interner = Interner::new();

    let a = interner.intern("foo");
    let b = interner.intern("foo");
    let c = interner.intern("bar");

    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_eq!(interner.resolve(a), "foo");
    assert_eq!(interner.resolve(c), "bar");
}

#[test]
fn def_type_round_trips_by_def_id() {
    let mut ctx = TypeAnalysisBuilder::new();
    let def_id = DefId::from_raw(0);

    ctx.set_def_type(def_id, TYPE_NODE);

    let analysis = ctx.finish();
    assert_eq!(analysis.def_type(def_id), Some(TYPE_NODE));
    assert_eq!(analysis.def_type(DefId::from_raw(1)), None);
}
