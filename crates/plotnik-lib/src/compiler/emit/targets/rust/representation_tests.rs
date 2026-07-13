use std::collections::BTreeMap;

use super::representation::TypeFacts;
use crate::compiler::analyze::types::RootExtent;
use crate::compiler::analyze::types::type_analysis::{TypeAnalysis, TypeAnalysisBuilder};
use crate::compiler::analyze::types::type_shape::{
    RecordField, TYPE_NODE, TYPE_VOID, TypeId, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::core::Interner;

struct Fixture {
    types: TypeAnalysis,
    ref_ty: TypeId,
    /// The recursive definition's own output — the item declaration the ref
    /// is rendered inside.
    item_ty: TypeId,
}

fn record_def(builder: &mut TypeAnalysisBuilder, def_id: DefId, type_id: TypeId) {
    builder.record_def_output(def_id, type_id);
    builder.record_def_root_extent(def_id, RootExtent::SingleNode);
}

fn borrows_any(facts: &TypeFacts, ty: TypeId) -> bool {
    let usage = facts.lifetime_usage(ty);
    usage.tree || usage.source
}

/// One definition whose output is an enum with a `Ref` back to itself, the
/// ref sitting behind the given wrapper chain.
fn recursive_def(wrap: impl FnOnce(&mut TypeAnalysisBuilder, TypeId) -> TypeId) -> Fixture {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let def = DefId::from_raw(0);

    let ref_ty = builder.intern_type(TypeShape::Ref(def));
    let payload_ty = wrap(&mut builder, ref_ty);
    let payload = builder.intern_record(BTreeMap::from([(
        interner.intern("inner"),
        RecordField::new(payload_ty),
    )]));
    let variants = BTreeMap::from([
        (interner.intern("Leaf"), TYPE_VOID),
        (interner.intern("Rec"), payload),
    ]);
    let enum_ty = builder.intern_type(TypeShape::Variant(variants));
    record_def(&mut builder, def, enum_ty);

    Fixture {
        types: builder.finish(),
        ref_ty,
        item_ty: enum_ty,
    }
}

#[test]
fn direct_recursive_ref_is_boxed_in_its_own_item() {
    let fx = recursive_def(|_, ref_ty| ref_ty);

    let facts = TypeFacts::compute(&fx.types);

    assert!(facts.is_boxed_in(fx.item_ty, fx.ref_ty));
}

#[test]
fn ref_under_option_is_boxed() {
    let fx = recursive_def(|builder, ref_ty| builder.intern_type(TypeShape::Option(ref_ty)));

    let facts = TypeFacts::compute(&fx.types);

    assert!(facts.is_boxed_in(fx.item_ty, fx.ref_ty));
}

/// The flagship occurrence-precision case: one interned `Ref` node used both
/// inside its target's own (recursive) declaration and from an off-cycle
/// item. Only the on-cycle rendering boxes; keying on the ref node alone
/// would drag the box into the off-cycle item too.
#[test]
fn shared_ref_node_boxes_only_inside_the_cycle() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let expr_def = DefId::from_raw(0);
    let top_def = DefId::from_raw(1);

    let ref_ty = builder.intern_type(TypeShape::Ref(expr_def));
    let payload = builder.intern_record(BTreeMap::from([(
        interner.intern("inner"),
        RecordField::new(ref_ty),
    )]));
    let variants = BTreeMap::from([
        (interner.intern("Leaf"), TYPE_VOID),
        (interner.intern("Rec"), payload),
    ]);
    let enum_ty = builder.intern_type(TypeShape::Variant(variants));
    record_def(&mut builder, expr_def, enum_ty);
    let top_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("expr"),
        RecordField::new(ref_ty),
    )]));
    record_def(&mut builder, top_def, top_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(facts.is_boxed_in(enum_ty, ref_ty));
    assert!(!facts.is_boxed_in(top_struct, ref_ty));
}

/// A cycle whose only closing path runs through an array is not a by-value
/// cycle: `Vec` already indirects, so the by-value closure stops at the
/// array and neither declaration boxes.
#[test]
fn cycle_through_an_array_boxes_nothing() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let a_def = DefId::from_raw(0);
    let b_def = DefId::from_raw(1);

    let ref_to_b = builder.intern_type(TypeShape::Ref(b_def));
    let list_of_b = builder.intern_type(TypeShape::Array {
        element: ref_to_b,
        non_empty: false,
    });
    let a_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("items"),
        RecordField::new(list_of_b),
    )]));
    record_def(&mut builder, a_def, a_struct);
    let ref_to_a = builder.intern_type(TypeShape::Ref(a_def));
    let b_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("parent"),
        RecordField::new(ref_to_a),
    )]));
    record_def(&mut builder, b_def, b_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    // `B.parent: A` is by-value, but `A` reaches back only through its
    // array, so no by-value cycle closes through `B`'s declaration. (The
    // `Vec<B>` occurrence inside `A` never even asks: renderers drop the
    // cut context under arrays.)
    assert!(!facts.is_boxed_in(b_struct, ref_to_a));
}

#[test]
fn ref_from_outside_the_cycle_is_not_boxed() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let leaf_def = DefId::from_raw(0);
    let top_def = DefId::from_raw(1);

    let leaf_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("id"),
        RecordField::new(TYPE_NODE),
    )]));
    record_def(&mut builder, leaf_def, leaf_struct);
    let ref_ty = builder.intern_type(TypeShape::Ref(leaf_def));
    let top_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("leaf"),
        RecordField::new(ref_ty),
    )]));
    record_def(&mut builder, top_def, top_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(!facts.is_boxed_in(top_struct, ref_ty));
}

/// Every declaration a genuine by-value cycle passes through cuts its ref
/// edge — deliberately *not* a minimal cut, which would be order-dependent;
/// boxing every on-cycle edge keeps the decision local and stable.
#[test]
fn mutual_recursion_boxes_both_edges() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let a_def = DefId::from_raw(0);
    let b_def = DefId::from_raw(1);

    let ref_to_b = builder.intern_type(TypeShape::Ref(b_def));
    let opt_ref_to_b = builder.intern_type(TypeShape::Option(ref_to_b));
    let a_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("b"),
        RecordField::new(opt_ref_to_b),
    )]));
    record_def(&mut builder, a_def, a_struct);
    let ref_to_a = builder.intern_type(TypeShape::Ref(a_def));
    let opt_ref_to_a = builder.intern_type(TypeShape::Option(ref_to_a));
    let b_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("a"),
        RecordField::new(opt_ref_to_a),
    )]));
    record_def(&mut builder, b_def, b_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(facts.is_boxed_in(b_struct, ref_to_a));
    assert!(facts.is_boxed_in(a_struct, ref_to_b));
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
    let enum_ty = builder.intern_type(TypeShape::Variant(variants));
    let holder = builder.intern_record(BTreeMap::from([(
        interner.intern("state"),
        RecordField::new(enum_ty),
    )]));
    record_def(&mut builder, def, holder);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(!borrows_any(&facts, enum_ty));
    assert!(!borrows_any(&facts, holder));
}

#[test]
fn custom_node_alias_carries_tree_lifetime_into_its_owner() {
    let mut interner = Interner::new();
    let mut builder = TypeAnalysisBuilder::new();
    let def = DefId::from_raw(0);

    let custom = builder.intern_type(TypeShape::Custom(interner.intern("Identifier")));
    let holder = builder.intern_record(BTreeMap::from([(
        interner.intern("name"),
        RecordField::new(custom),
    )]));
    record_def(&mut builder, def, holder);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(facts.lifetime_usage(custom).tree);
    assert!(facts.lifetime_usage(holder).tree);
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
    let a_struct = builder.intern_record(BTreeMap::from([(
        interner.intern("items"),
        RecordField::new(list_of_b),
    )]));
    record_def(&mut builder, a_def, a_struct);
    let ref_to_a = builder.intern_type(TypeShape::Ref(a_def));
    let opt_ref_to_a = builder.intern_type(TypeShape::Option(ref_to_a));
    let b_struct = builder.intern_record(BTreeMap::from([
        (interner.intern("name"), RecordField::new(TYPE_NODE)),
        (interner.intern("parent"), RecordField::new(opt_ref_to_a)),
    ]));
    record_def(&mut builder, b_def, b_struct);
    let types = builder.finish();

    let facts = TypeFacts::compute(&types);

    assert!(borrows_any(&facts, a_struct));
    assert!(borrows_any(&facts, b_struct));
}
