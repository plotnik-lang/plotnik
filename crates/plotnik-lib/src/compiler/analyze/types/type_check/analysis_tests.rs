use std::collections::BTreeMap;

use crate::compiler::analyze::types::RootExtent;
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{
    DefinitionOutput, RecordField, TYPE_NODE, TypeShape,
};
use crate::compiler::ids::DefId;
use crate::core::Interner;

#[test]
fn type_interning_deduplicates() {
    let mut ctx = TypeAnalysisBuilder::new();

    let id1 = ctx.intern_type(TypeShape::Node);
    let id2 = ctx.intern_type(TypeShape::Node);

    assert_eq!(id1, id2);
    assert_eq!(id1, TYPE_NODE);
}

#[test]
fn option_interning_is_idempotent() {
    let mut ctx = TypeAnalysisBuilder::new();

    let option = ctx.intern_type(TypeShape::Option(TYPE_NODE));
    let nested = ctx.intern_type(TypeShape::Option(option));

    assert_eq!(nested, option);
}

#[test]
fn option_interning_preserves_an_option_declaration_reference() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let definition = DefId::from_raw(0);
    ctx.declare_definitions([(definition, interner.intern("Definition"))]);
    let option = ctx.intern_option(TYPE_NODE);
    ctx.record_def_output(definition, DefinitionOutput::Value(option));
    ctx.record_def_root_extent(definition, RootExtent::SingleNode);
    let reference = ctx.definition_ref(definition);

    let wrapped = ctx.intern_option(reference);

    assert_eq!(wrapped, reference);
}

#[test]
fn record_bodies_have_distinct_ids_but_compare_structurally() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();

    let x_sym = interner.intern("x");
    let mut fields = BTreeMap::new();
    fields.insert(x_sym, RecordField::new(TYPE_NODE));

    let id1 = ctx.intern_type(TypeShape::Record(fields.clone()));
    let id2 = ctx.intern_type(TypeShape::Record(fields));

    assert_ne!(id1, id2);
    assert!(ctx.types_structurally_equal(id1, id2));
}

#[test]
fn distinct_record_declarations_are_nominal() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let field = interner.intern("field");
    let left = DefId::from_raw(0);
    let right = DefId::from_raw(1);
    ctx.declare_definitions([
        (left, interner.intern("Left")),
        (right, interner.intern("Right")),
    ]);
    let left_body = ctx.intern_record(BTreeMap::from([(field, RecordField::new(TYPE_NODE))]));
    let right_body = ctx.intern_record(BTreeMap::from([(field, RecordField::new(TYPE_NODE))]));
    ctx.record_def_output(left, DefinitionOutput::Value(left_body));
    ctx.record_def_output(right, DefinitionOutput::Value(right_body));
    let left_ref = ctx.definition_ref(left);
    let right_ref = ctx.definition_ref(right);

    assert!(!ctx.types_structurally_equal(left_ref, right_ref));
}

#[test]
fn transparent_definition_aliases_compare_by_body() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let left = DefId::from_raw(0);
    let right = DefId::from_raw(1);
    ctx.declare_definitions([
        (left, interner.intern("Left")),
        (right, interner.intern("Right")),
    ]);
    ctx.record_def_output(left, DefinitionOutput::Value(TYPE_NODE));
    ctx.record_def_output(right, DefinitionOutput::Value(TYPE_NODE));
    let left_ref = ctx.definition_ref(left);
    let right_ref = ctx.definition_ref(right);

    assert!(ctx.types_structurally_equal(left_ref, right_ref));
    assert!(ctx.types_structurally_equal(left_ref, TYPE_NODE));
}
