use std::collections::BTreeMap;

use super::*;

#[test]
fn builtin_types_have_correct_ids() {
    let ctx = TypeContext::new();

    assert_eq!(ctx.get_type(TYPE_VOID), Some(&TypeShape::Void));
    assert_eq!(ctx.get_type(TYPE_NODE), Some(&TypeShape::Node));
    assert_eq!(ctx.get_type(TYPE_STRING), Some(&TypeShape::String));
}

#[test]
fn type_interning_deduplicates() {
    let mut ctx = TypeContext::new();

    let id1 = ctx.intern_type(TypeShape::Node);
    let id2 = ctx.intern_type(TypeShape::Node);

    assert_eq!(id1, id2);
    assert_eq!(id1, TYPE_NODE);
}

#[test]
fn struct_types_intern_correctly() {
    let mut ctx = TypeContext::new();
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
fn def_type_by_name() {
    let mut ctx = TypeContext::new();
    let mut interner = Interner::new();

    ctx.set_def_type_by_name(&mut interner, "Query", TYPE_NODE);
    assert_eq!(
        ctx.get_def_type_by_name(&interner, "Query"),
        Some(TYPE_NODE)
    );
    assert_eq!(ctx.get_def_type_by_name(&interner, "Missing"), None);
}

#[test]
fn register_def_returns_stable_id() {
    let mut ctx = TypeContext::new();
    let mut interner = Interner::new();

    let id1 = ctx.register_def(&mut interner, "Foo");
    let id2 = ctx.register_def(&mut interner, "Bar");
    let id3 = ctx.register_def(&mut interner, "Foo"); // duplicate

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    assert_eq!(ctx.def_name(&interner, id1), "Foo");
    assert_eq!(ctx.def_name(&interner, id2), "Bar");
}

#[test]
fn def_id_lookup() {
    let mut ctx = TypeContext::new();
    let mut interner = Interner::new();

    ctx.register_def(&mut interner, "Query");
    assert!(ctx.get_def_id(&interner, "Query").is_some());
    assert!(ctx.get_def_id(&interner, "Missing").is_none());
}
