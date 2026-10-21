use std::thread;

use indoc::indoc;
use plotnik_compiler::bytecode::Module;
use plotnik_compiler::{BytecodeConfig, Colors, NoopTracer, QueryBuilder, RuntimeError, VM, Value};

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
                let vm = VM::builder(&source, &tree).build();
                let mut tracer = NoopTracer;
                let result = vm.execute_with(&module, &entry, &mut tracer);
                match result {
                    Ok(_) => "matched",
                    Err(RuntimeError::NoMatch) => "no-match",
                    Err(RuntimeError::CallFrame(_)) => "call-frame",
                }
            })
            .expect("spawn deep-backtrack thread");
        handle.join().expect("deep-backtrack thread did not abort")
    });

    assert_eq!(outcome, "no-match", "expected a clean no-match outcome");
}

#[test]
fn deep_value_render_and_drop_do_not_overflow_native_stack() {
    const VALUE_DEPTH: usize = 200_000;

    let len = thread::scope(|scope| {
        let handle = thread::Builder::new()
            .name("deep-value".into())
            .stack_size(STACK_SIZE)
            .spawn_scoped(scope, || {
                let mut value = Value::Absent;
                for _ in 0..VALUE_DEPTH {
                    value = Value::Record(vec![("inner", value)]);
                }
                let rendered = value.format(false, Colors::new(false));
                rendered.len()
            })
            .expect("spawn deep-value thread");
        handle.join().expect("deep-value thread did not abort")
    });

    assert!(
        len > VALUE_DEPTH,
        "expected rendered output longer than {VALUE_DEPTH}, saw {len}"
    );
}
