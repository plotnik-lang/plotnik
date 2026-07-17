use std::collections::BTreeMap;

use super::unify::{UnifyError, unify_alternative_flows};
use crate::compiler::Diagnostics;
use crate::compiler::analyze::types::inference_flow::{
    CaptureId, InferredField, InferredFieldFlow,
};
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{
    PatternFlow, PatternShape, RecordField, TYPE_NODE, TYPE_TEXT, TypeId, TypeShape,
};
use crate::compiler::diagnostics::SourceId;
use crate::compiler::diagnostics::span::Span;
use crate::compiler::parse::{ParseConfig, Pattern, parse_lossless};
use crate::core::{Interner, Symbol};

#[test]
fn unify_suppresses_uncaptured_value() {
    // A pending value nothing captures (a bare reference alternative) is suppressed,
    // not an error: `[(Foo) (bar)]` is a plain structural alternation.
    let mut ctx = TypeAnalysisBuilder::new();

    let result = unify_alternative_flows(
        &mut ctx,
        [
            (
                Some(pattern("(Foo)")),
                PatternShape::new(
                    crate::compiler::analyze::types::RootExtent::SingleNode,
                    PatternFlow::Value(TYPE_NODE),
                ),
            ),
            (Some(pattern("(bar)")), PatternShape::no_value()),
        ],
    );

    assert!(matches!(result, Ok(None)));
}

#[test]
fn record_field_absence_is_an_idempotent_option() {
    let mut ctx = TypeAnalysisBuilder::new();
    let mut interner = Interner::new();
    let name = interner.intern("name");
    let first = field_shape(&mut ctx, name, TYPE_NODE, 0);
    let second = field_shape(&mut ctx, name, TYPE_NODE, 1);

    let merged = unify_alternative_flows(
        &mut ctx,
        [
            (Some(pattern("(a)")), PatternShape::no_value()),
            (Some(pattern("(b)")), first),
            (Some(pattern("(c)")), second),
        ],
    )
    .expect("a missing record alternative is compatible")
    .expect("record alternatives produce a field flow");

    let final_type = ctx.in_progress().expect_record_fields(merged.type_id)[&name].final_type;
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
    let left = field_shape(&mut ctx, name, optional_node, 0);
    let right = field_shape(&mut ctx, name, optional_text, 1);

    let error = unify_alternative_flows(
        &mut ctx,
        [(Some(pattern("(a)")), left), (Some(pattern("(b)")), right)],
    )
    .expect_err("different optional element types are incompatible");

    assert!(matches!(
        error,
        UnifyError::IncompatibleFieldTypes {
            field,
            left_type,
            right_type,
            ..
        } if field == name && left_type == optional_node && right_type == optional_text
    ));
}

fn field_shape(
    ctx: &mut TypeAnalysisBuilder,
    name: Symbol,
    type_id: TypeId,
    capture_index: usize,
) -> PatternShape {
    let info = RecordField::new(type_id);
    let field = InferredField::capture(
        info,
        CaptureId::from_index(capture_index),
        Span::new(SourceId::default(), rowan::TextRange::default()),
        Span::new(SourceId::default(), rowan::TextRange::default()),
    );
    let fields = BTreeMap::from([(name, field)]);
    let record = ctx.intern_record(BTreeMap::from([(name, info)]));
    PatternShape::fields(
        crate::compiler::analyze::types::RootExtent::SingleNode,
        InferredFieldFlow::new(record, fields),
    )
}

fn pattern(source: &str) -> Pattern {
    let query = format!("Q = {source}");
    let mut diagnostics = Diagnostics::new();
    let root = parse_lossless(
        &query,
        SourceId::default(),
        &mut diagnostics,
        ParseConfig::default(),
    )
    .expect("test pattern parses within resource limits");
    assert!(!diagnostics.has_errors(), "test pattern must be valid");
    root.defs()
        .next()
        .and_then(|definition| definition.body())
        .expect("test definition contains a pattern")
}
