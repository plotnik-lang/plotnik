mod support;

use plotnik_lib::bytecode::{Entrypoint, Module};
use plotnik_lib::{
    BytecodeConfig, Limit, NoopTracer, QueryBuilder, RuntimeError, RuntimeLimitSpec, VM,
};

fn compile(src: &str) -> Module {
    let compiled = QueryBuilder::from_inline(src)
        .compile(support::javascript_grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("valid query emits module")
}

fn module_and_entry() -> (Module, Entrypoint) {
    let module = compile("Q = (program (expression_statement (identifier) @id))");
    let entry = module.entrypoint("Q").expect("Q entrypoint exists");
    (module, entry)
}

#[test]
fn execute_with_stats_reports_success_usage() {
    let (module, entry) = module_and_entry();
    let source = "x";
    let tree = support::parse_javascript(source);
    let vm = VM::builder(source, &tree).build();
    let mut tracer = NoopTracer;

    let (result, stats) = vm.execute_with_stats(&module, &entry, &mut tracer);

    assert!(result.is_ok(), "run should match");
    assert!(stats.fuel_used > 0);
    assert!(stats.heap_high_water > 0);
}

#[test]
fn execute_with_stats_reports_fuel_usage() {
    let (module, entry) = module_and_entry();
    let source = "x";
    let tree = support::parse_javascript(source);
    let vm = VM::builder(source, &tree)
        .limits(RuntimeLimitSpec {
            fuel_limit: Limit::Of(1),
            memory: Limit::Unbounded,
        })
        .build();
    let mut tracer = NoopTracer;

    let (result, stats) = vm.execute_with_stats(&module, &entry, &mut tracer);

    assert!(matches!(result, Err(RuntimeError::OutOfFuel(1))));
    assert_eq!(stats.fuel_used, 1);
    assert!(stats.heap_high_water > 0);
}
