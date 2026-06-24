use std::collections::BTreeMap;

use super::context::TypeAnalysisBuilder;
use super::def_id::Interner;
use super::types::{FieldInfo, TYPE_NODE, TypeShape};

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
