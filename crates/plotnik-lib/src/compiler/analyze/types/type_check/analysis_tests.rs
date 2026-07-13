use std::collections::BTreeMap;

use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{RecordField, TYPE_NODE, TypeShape};
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
fn record_types_are_nominal() {
    // Records mint a fresh id per occurrence: two definitions with identical
    // capture profiles are distinct named types. Structural equality is a
    // separate relation used by unification.
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
