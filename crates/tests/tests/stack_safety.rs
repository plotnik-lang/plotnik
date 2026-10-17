use std::thread;

use indoc::indoc;
use plotnik_compiler::bytecode::Module;
use plotnik_compiler::{
    BytecodeConfig, Limit, NoopTracer, QueryBuilder, RuntimeError, RuntimeLimitSpec, VM,
};

mod support;

const DEPTH: usize = 10_000;

const STACK_SIZE: usize = 256 * 1024;

const QUERY: &str = indoc! {"
    Rec = [Leaf: (number) Deep: (unary_expression (Rec))]
    Top = (program (expression_statement (Rec)))
"};

fn compile(query: &str) -> Module {
    let compiled = QueryBuilder::from_inline(query)
        .compile(support::javascript_grammar())
        .expect("query compiles");
    assert!(
        compiled.is_valid(),
        "{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("query emits a module")
}

#[test]
fn deep_backtrack_does_not_overflow_native_stack() {
    let module = compile(QUERY);

    let source = format!("{}x", "!".repeat(DEPTH));
    let tree = support::parse_javascript(&source);

    let entry = module.entrypoint("Top").expect("Top is an entrypoint");

    let outcome = thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("deep-backtrack".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                let vm = VM::builder(&source, &tree)
                    .limits(RuntimeLimitSpec {
                        fuel_limit: Limit::Unbounded,
                        memory: Limit::Unbounded,
                    })
                    .build();
                let mut tracer = NoopTracer;
                let result = vm.execute_with(&module, &entry, &mut tracer);
                match result {
                    Ok(_) => "matched",
                    Err(RuntimeError::NoMatch) => "no-match",
                    Err(RuntimeError::Limit(_)) => "limit",
                }
            })
            .expect("spawn deep-backtrack thread");
        handle.join().expect("deep-backtrack thread did not abort")
    });

    assert_eq!(outcome, "no-match", "expected a clean no-match outcome");
}

#[test]
fn deep_value_render_and_drop_do_not_overflow_native_stack() {
    let module = compile(
        "Rec = [Leaf: (number) @n Deep: (unary_expression (Rec) @inner)] Top = (program (expression_statement (Rec) @value))",
    );
    let source = format!("{}1", "!".repeat(DEPTH));
    let tree = support::parse_javascript(&source);
    let entry = module.entrypoint("Top").unwrap();

    let rendered = thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("deep-value".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                let document = VM::builder(&source, &tree)
                    .limits(RuntimeLimitSpec {
                        fuel_limit: Limit::Unbounded,
                        memory: Limit::Unbounded,
                    })
                    .build()
                    .execute(&module, &entry)
                    .unwrap();
                let mut rendered = String::new();
                document
                    .write_json(plotnik_runtime::json::Style::COMPACT, &mut rendered)
                    .unwrap();
                rendered
            })
            .expect("spawn deep-value thread");
        handle.join().expect("deep-value thread did not abort")
    });

    let mut expected = String::from("{\"value\":");
    for _ in 0..DEPTH {
        expected.push_str("{\"$tag\":\"Deep\",\"$data\":{\"inner\":");
    }
    expected.push_str(&format!("{{\"$tag\":\"Leaf\",\"$data\":{{\"n\":{{\"kind\":\"number\",\"text\":\"1\",\"span\":[{DEPTH}, {}]}}}}}}", DEPTH + 1));
    for _ in 0..DEPTH {
        expected.push_str("}}");
    }
    expected.push('}');
    assert_eq!(rendered, expected);
}
