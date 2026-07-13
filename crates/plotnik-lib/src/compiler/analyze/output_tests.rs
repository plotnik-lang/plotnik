use std::collections::BTreeMap;

use crate::compiler::analyze::types::RootExtent;
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::analyze::types::type_shape::{DefinitionOutput, RecordField, TYPE_NODE};
use crate::compiler::ids::DefId;
use crate::compiler::test_utils::synthetic_grammar;
use crate::compiler::{QueryBuilder, TypeScriptCodegenConfig};
use crate::core::Interner;

use super::output::{CaptureLayout, OutputSchemaError, collect_ordered_types};

#[test]
fn capture_layout_assigns_one_absolute_member_sequence() {
    let mut interner = Interner::new();
    let mut types = TypeAnalysisBuilder::new();
    let child = types.intern_record(BTreeMap::from([(
        interner.intern("value"),
        RecordField::new(TYPE_NODE),
    )]));
    let parent = types.intern_record(BTreeMap::from([
        (interner.intern("child"), RecordField::new(child)),
        (interner.intern("name"), RecordField::new(TYPE_NODE)),
    ]));
    let def = DefId::from_raw(0);
    types.record_def_output(def, DefinitionOutput::Value(parent));
    types.record_def_root_extent(def, RootExtent::SingleNode);
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
                RecordField::new(TYPE_NODE),
            )
        })
        .collect();
    let output = types.intern_record(fields);
    let def = DefId::from_raw(0);
    types.record_def_output(def, DefinitionOutput::Value(output));
    types.record_def_root_extent(def, RootExtent::SingleNode);
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
                    RecordField::new(TYPE_NODE),
                )
            })
            .collect();
        ordered.push(types.intern_record(fields));
    }
    let fields = (0..10)
        .map(|field| {
            (
                interner.intern(&format!("overflow_field_{field}")),
                RecordField::new(TYPE_NODE),
            )
        })
        .collect();
    ordered.push(types.intern_record(fields));
    let types = types.finish();

    let error = CaptureLayout::build(&types, &ordered)
        .expect_err("65,545 members exceed the capture layout limit");

    assert_eq!(error, OutputSchemaError::Members(65_545));
}

#[test]
fn output_items_include_only_reachable_fragments() {
    let source = emitted_types(
        "Unused = (number)*\n\
         Row = (array (identifier) @value)\n\
         Rows = (Row)*\n\
         Q = (program (expression_statement (array (Rows) @rows)))",
    );

    assert!(source.contains("export interface Row"), "{source}");
    assert!(source.contains("export type Rows = Row[];"), "{source}");
    assert!(source.contains("rows: Rows;"), "{source}");
    assert!(!source.contains("Unused"), "{source}");
}

#[test]
fn scalar_capture_does_not_publish_its_structured_fragment() {
    let source = emitted_types(
        "Chunk = {(comment) @comment (expression_statement) @statement}\n\
         Q = (program (Chunk) @text :: str)",
    );

    assert!(source.contains("text: string;"), "{source}");
    assert!(!source.contains("Chunk"), "{source}");
}

#[test]
fn mutually_recursive_items_are_collected_once() {
    let source = emitted_types(
        "A = [Base: (identifier) @id Nest: (array (B) @b)]\n\
         B = [Leaf: (number) @n Wrap: (array (A) @a)]\n\
         Q = (program (expression_statement (A) @root))",
    );

    assert_eq!(source.matches("export type A =").count(), 1, "{source}");
    assert_eq!(source.matches("export type B =").count(), 1, "{source}");
    assert_eq!(source.matches("export interface Q").count(), 1, "{source}");
}

fn emitted_types(src: &str) -> String {
    let compiled = QueryBuilder::from_inline(src)
        .compile(synthetic_grammar())
        .expect("test query compiles");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );

    compiled
        .emit_types(TypeScriptCodegenConfig::new())
        .expect("TypeScript type emission answers")
        .into_artifact()
        .expect("valid query emits TypeScript types")
        .source()
        .to_owned()
}
