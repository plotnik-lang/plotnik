use std::collections::BTreeMap;

use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{Arity, FieldInfo, TYPE_NODE};
use crate::compiler::ids::DefId;
use crate::core::Interner;

use super::output::{CaptureLayout, OutputSchemaError, collect_ordered_types};

#[test]
fn capture_layout_assigns_one_absolute_member_sequence() {
    let mut interner = Interner::new();
    let mut types = TypeAnalysisBuilder::new();
    let child = types.intern_struct(BTreeMap::from([(
        interner.intern("value"),
        FieldInfo::required(TYPE_NODE),
    )]));
    let parent = types.intern_struct(BTreeMap::from([
        (interner.intern("child"), FieldInfo::required(child)),
        (interner.intern("name"), FieldInfo::required(TYPE_NODE)),
    ]));
    let def = DefId::from_raw(0);
    types.record_def_output(def, parent);
    types.record_def_arity(def, Arity::One);
    let types = types.finish();

    let ordered = collect_ordered_types(&types);
    let layout = CaptureLayout::build(&types, &ordered).expect("small layout fits");

    let child_scope = layout.scope(child).expect("child is reachable");
    let parent_scope = layout.scope(parent).expect("parent is reachable");
    assert_eq!(child_scope.base(), 0);
    assert_eq!(child_scope.absolute_index(0), 0);
    assert_eq!(parent_scope.base(), 1);
    assert_eq!(parent_scope.absolute_index(1), 2);
    assert_eq!(layout.member_count(), 3);
}

#[test]
fn capture_layout_accepts_256_fields() {
    let mut interner = Interner::new();
    let mut types = TypeAnalysisBuilder::new();
    let fields = (0..=u8::MAX)
        .map(|index| {
            (
                interner.intern(&format!("field_{index}")),
                FieldInfo::required(TYPE_NODE),
            )
        })
        .collect();
    let output = types.intern_struct(fields);
    let def = DefId::from_raw(0);
    types.record_def_output(def, output);
    types.record_def_arity(def, Arity::One);
    let types = types.finish();

    let layout = CaptureLayout::build(&types, &collect_ordered_types(&types))
        .expect("per-scope widths belong to bytecode emission");

    assert_eq!(layout.member_count(), 256);
}

#[test]
fn capture_layout_reports_the_actual_total_member_count() {
    let mut interner = Interner::new();
    let mut types = TypeAnalysisBuilder::new();
    let mut ordered = Vec::new();
    for scope in 0..257 {
        let fields = (0..u8::MAX)
            .map(|field| {
                (
                    interner.intern(&format!("scope_{scope}_field_{field}")),
                    FieldInfo::required(TYPE_NODE),
                )
            })
            .collect();
        ordered.push(types.intern_struct(fields));
    }
    let fields = (0..10)
        .map(|field| {
            (
                interner.intern(&format!("overflow_field_{field}")),
                FieldInfo::required(TYPE_NODE),
            )
        })
        .collect();
    ordered.push(types.intern_struct(fields));
    let types = types.finish();

    let error = CaptureLayout::build(&types, &ordered)
        .expect_err("65,545 members exceed the capture layout limit");

    assert_eq!(error, OutputSchemaError::Members(65_545));
}
