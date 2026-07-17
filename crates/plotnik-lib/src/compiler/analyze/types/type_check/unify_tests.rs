use std::collections::BTreeMap;

use super::unify::{UnifyError, unify_flow};
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{
    PatternFlow, RecordField, TYPE_NODE, TYPE_TEXT, TypeShape,
};
use crate::core::Interner;

#[test]
fn unify_suppresses_uncaptured_value() {
    // A pending value nothing captures (a bare reference alternative) is suppressed,
    // not an error: `[(Foo) (bar)]` is a plain structural alternation.
    let mut ctx = TypeAnalysisBuilder::new();

    let result = unify_flow(
        &mut ctx,
        PatternFlow::Value(TYPE_NODE),
        PatternFlow::NoValue,
    );

    assert!(matches!(result, Ok(PatternFlow::NoValue)));
}

#[test]
fn record_field_absence_is_an_idempotent_option() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let name = interner.intern("name");
    let required = ctx.intern_record(BTreeMap::from([(name, RecordField::new(TYPE_NODE))]));

    let absent = unify_flow(
        &mut ctx,
        PatternFlow::NoValue,
        PatternFlow::Fields(required),
    )
    .expect("a missing record alternative is compatible");
    let merged = unify_flow(&mut ctx, absent, PatternFlow::Fields(required))
        .expect("an option field is compatible with its inner type");

    let PatternFlow::Fields(record) = merged else {
        panic!("record alternatives produce fields")
    };
    let final_type = ctx.in_progress().expect_record_fields(record)[&name].final_type;
    assert_eq!(
        ctx.in_progress().type_shape(final_type),
        Some(&TypeShape::Option(TYPE_NODE)),
    );
}

#[test]
fn incompatible_error_preserves_the_complete_field_types() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let name = interner.intern("value");
    let optional_node = ctx.intern_option(TYPE_NODE);
    let optional_text = ctx.intern_option(TYPE_TEXT);
    let left = ctx.intern_record(BTreeMap::from([(name, RecordField::new(optional_node))]));
    let right = ctx.intern_record(BTreeMap::from([(name, RecordField::new(optional_text))]));

    let error = unify_flow(
        &mut ctx,
        PatternFlow::Fields(left),
        PatternFlow::Fields(right),
    )
    .expect_err("different optional element types are incompatible");

    assert!(matches!(
        error,
        UnifyError::IncompatibleTypes {
            field,
            left_type,
            right_type,
        } if field == name && left_type == optional_node && right_type == optional_text
    ));
}
