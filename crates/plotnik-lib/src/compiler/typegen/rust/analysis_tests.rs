use std::collections::BTreeMap;

use super::analysis::TypeFacts;
use crate::compiler::analyze::types::type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
use crate::compiler::analyze::types::type_shape::{
    FieldInfo, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::core::Interner;

struct Fixture {
    types: TypeAnalysis,
    ref_ty: TypeId,
}

/// One definition whose output is an enum with a `Ref` back to itself, the
/// ref sitting behind the given wrapper chain.
fn recursive_def(wrap: impl FnOnce(&mut TypeAnalysisBuilder, TypeId) -> TypeId) -> Fixture {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let def = DefId::from_raw(0);

    let ref_ty = builder.intern_type(TypeShape::Ref(def));
    let payload_ty = wrap(&mut builder, ref_ty);
    let payload = builder.intern_struct(BTreeMap::from([(
        interner.intern("inner"),
        FieldInfo::required(payload_ty),
    )]));
    let variants = BTreeMap::from([
        (interner.intern("Leaf"), TYPE_VOID),
        (interner.intern("Rec"), payload),
    ]);
    let enum_ty = builder.intern_type(TypeShape::Enum(variants));
    builder.record_def_output(def, enum_ty);

    Fixture {
        types: builder.finish(),
        ref_ty,
    }
}

#[test]
fn direct_recursive_ref_is_boxed() {
    let fx = recursive_def(|_, ref_ty| ref_ty);

    let facts = TypeFacts::compute(&fx.types);

    assert!(facts.is_boxed(fx.ref_ty));
}

#[test]
fn ref_under_array_is_not_boxed() {
    let fx = recursive_def(|builder, ref_ty| {
        builder.intern_type(TypeShape::Array {
            element: ref_ty,
            non_empty: false,
        })
    });

    let facts = TypeFacts::compute(&fx.types);

    assert!(!facts.is_boxed(fx.ref_ty));
}

#[test]
fn ref_under_optional_is_boxed() {
    let fx = recursive_def(|builder, ref_ty| builder.intern_type(TypeShape::Optional(ref_ty)));

    let facts = TypeFacts::compute(&fx.types);

    assert!(facts.is_boxed(fx.ref_ty));
}

#[test]
fn ref_from_outside_the_cycle_is_not_boxed() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let leaf_def = DefId::from_raw(0);
    let top_def = DefId::from_raw(1);

    let leaf_struct = builder.intern_struct(BTreeMap::from([(
        interner.intern("id"),
        FieldInfo::required(TYPE_NODE),
    )]));
    builder.record_def_output(leaf_def, leaf_struct);
    let ref_ty = builder.intern_type(TypeShape::Ref(leaf_def));
    let top_struct = builder.intern_struct(BTreeMap::from([(
        interner.intern("leaf"),
        FieldInfo::required(ref_ty),
    )]));
    builder.record_def_output(top_def, top_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(!facts.is_boxed(ref_ty));
}

#[test]
fn mutual_recursion_boxes_both_edges() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let a_def = DefId::from_raw(0);
    let b_def = DefId::from_raw(1);

    let ref_to_b = builder.intern_type(TypeShape::Ref(b_def));
    let opt_ref_to_b = builder.intern_type(TypeShape::Optional(ref_to_b));
    let a_struct = builder.intern_struct(BTreeMap::from([(
        interner.intern("b"),
        FieldInfo::required(opt_ref_to_b),
    )]));
    builder.record_def_output(a_def, a_struct);
    let ref_to_a = builder.intern_type(TypeShape::Ref(a_def));
    let opt_ref_to_a = builder.intern_type(TypeShape::Optional(ref_to_a));
    let b_struct = builder.intern_struct(BTreeMap::from([(
        interner.intern("a"),
        FieldInfo::required(opt_ref_to_a),
    )]));
    builder.record_def_output(b_def, b_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(facts.is_boxed(ref_to_a));
    assert!(facts.is_boxed(ref_to_b));
}

#[test]
fn node_free_enum_needs_no_lifetime() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let def = DefId::from_raw(0);

    let variants = BTreeMap::from([
        (interner.intern("On"), TYPE_VOID),
        (interner.intern("Off"), TYPE_VOID),
    ]);
    let enum_ty = builder.intern_type(TypeShape::Enum(variants));
    let holder = builder.intern_struct(BTreeMap::from([(
        interner.intern("state"),
        FieldInfo::required(enum_ty),
    )]));
    builder.record_def_output(def, holder);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(!facts.needs_lifetime(enum_ty));
    assert!(!facts.needs_lifetime(holder));
}

#[test]
fn lifetime_crosses_mutual_recursion() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let a_def = DefId::from_raw(0);
    let b_def = DefId::from_raw(1);

    // `A` has no node of its own; it holds one only through `B`.
    let ref_to_b = builder.intern_type(TypeShape::Ref(b_def));
    let list_of_b = builder.intern_type(TypeShape::Array {
        element: ref_to_b,
        non_empty: false,
    });
    let a_struct = builder.intern_struct(BTreeMap::from([(
        interner.intern("items"),
        FieldInfo::required(list_of_b),
    )]));
    builder.record_def_output(a_def, a_struct);
    let ref_to_a = builder.intern_type(TypeShape::Ref(a_def));
    let opt_ref_to_a = builder.intern_type(TypeShape::Optional(ref_to_a));
    let b_struct = builder.intern_struct(BTreeMap::from([
        (interner.intern("name"), FieldInfo::required(TYPE_NODE)),
        (interner.intern("parent"), FieldInfo::required(opt_ref_to_a)),
    ]));
    builder.record_def_output(b_def, b_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(facts.needs_lifetime(a_struct));
    assert!(facts.needs_lifetime(b_struct));
}
