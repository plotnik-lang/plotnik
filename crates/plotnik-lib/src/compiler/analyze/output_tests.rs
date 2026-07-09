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
fn capture_layout_owns_the_per_scope_width_check() {
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

    let error = CaptureLayout::build(&types, &collect_ordered_types(&types))
        .expect_err("256 fields exceed the capture-scope count");

    assert_eq!(error, OutputSchemaError::Fields(256));
}
