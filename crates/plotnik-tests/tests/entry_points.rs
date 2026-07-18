use indoc::indoc;
use plotnik_lib::bytecode::Module;
use plotnik_lib::{
    BytecodeConfig, BytecodeInspection, Colors, QueryBuilder, VM, Value, materialize_verified,
};

mod support;

const QUERY: &str = indoc! {"
    First = (program (expression_statement (identifier) @first))
    Second = (program (expression_statement (identifier) @second))
    Fragment = (identifier)?
"};

const ENTRY_POINTS: [(&str, &str); 2] = [("First", "first"), ("Second", "second")];

#[test]
fn all_entry_points_run_from_default_and_inspection_modules() {
    let compiled = QueryBuilder::from_inline(QUERY)
        .compile(support::javascript_grammar())
        .expect("query compiles");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map()),
    );

    let compiled_names: Vec<_> = compiled.entry_point_names().collect();
    assert_eq!(
        compiled_names,
        ENTRY_POINTS
            .iter()
            .map(|&(entry, _)| entry)
            .collect::<Vec<_>>(),
    );

    let default_module = emit_module(&compiled, BytecodeConfig::new());
    let inspection_module = emit_module(
        &compiled,
        BytecodeConfig::new().inspection(BytecodeInspection::Spans),
    );

    for module in [&default_module, &inspection_module] {
        let module_names: Vec<_> = module.entry_point_names().collect();
        assert_eq!(module_names, compiled_names);

        for &(entry, field) in &ENTRY_POINTS {
            assert_entry_result(module, entry, field);
        }
    }
}

fn emit_module(compiled: &plotnik_lib::CompiledQuery, config: BytecodeConfig) -> Module {
    compiled
        .emit(config)
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("valid query emits a module")
}

fn assert_entry_result(module: &Module, entry_name: &str, field_name: &str) {
    let source = "x;";
    let tree = support::parse_javascript(source);
    let entry = module
        .entry_point(entry_name)
        .expect("advertised entry point exists");
    let journal = VM::builder(source, &tree)
        .build()
        .execute(module, &entry)
        .expect("entry point matches");
    let value = materialize_verified(
        source,
        module,
        &entry,
        journal.output_events(),
        Colors::new(false),
    );

    let Value::Record(fields) = &value else {
        panic!("capturing entry point must produce a record");
    };
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].0, field_name);
    let Value::Node(node) = &fields[0].1 else {
        panic!("captured identifier must materialize as a node");
    };
    assert_eq!(node.kind, "identifier");
    assert_eq!(node.text, "x");
}
