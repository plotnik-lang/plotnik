use arborium_tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use crate::bytecode::{Entrypoint, Module};
use crate::compiler::test_utils::javascript_grammar;
use crate::{Limit, NoopTracer, QueryBuilder, RuntimeError, RuntimeLimitSpec, VM};

fn compile(src: &str) -> Module {
    let compiled = QueryBuilder::from_inline(src)
        .compile(javascript_grammar())
        .expect("query parsing should not exhaust fuel");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled.into_module().expect("valid query emits module")
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
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
    let tree = parse_js(source);
    let vm = VM::builder(source, &tree).build();
    let mut tracer = NoopTracer;

    let (result, stats) = vm.execute_with_stats(&module, &entry, &mut tracer);

    assert!(result.is_ok(), "run should match");
    assert!(stats.steps_used > 0);
    assert!(stats.heap_high_water > 0);
}

#[test]
fn execute_with_stats_reports_step_limit_usage() {
    let (module, entry) = module_and_entry();
    let source = "x";
    let tree = parse_js(source);
    let vm = VM::builder(source, &tree)
        .limits(RuntimeLimitSpec {
            steps: Limit::Of(1),
            memory: Limit::Unbounded,
        })
        .build();
    let mut tracer = NoopTracer;

    let (result, stats) = vm.execute_with_stats(&module, &entry, &mut tracer);

    assert!(matches!(result, Err(RuntimeError::StepLimitExceeded(1))));
    assert_eq!(stats.steps_used, 1);
    assert!(stats.heap_high_water > 0);
}
