//! Adversarial checks for the untrusted-text boundaries. Exact language and VM
//! behavior belongs in snapshots; these properties only require totality and
//! formatter idempotence across inputs too broad for a finite corpus.

use std::sync::OnceLock;

use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::FileFailurePersistence;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_lib::bytecode::Module;
use plotnik_lib::{
    BytecodeConfig, Colors, QueryBuilder, TypeScriptCodegenConfig, VM, Value, format_query,
    materialize_verified,
};

mod support;

const VALID_SMOKE_QUERIES: &[&str] = &[
    "Q=(program)@root",
    "Q=(program(expression_statement(_)@value)+@items)",
    "Q=(program(expression_statement[A:(identifier)@a B:(number)@b]@choice))",
    "Q=(_)",
];

fn property_config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

fn try_compile_smoke(query: &str) {
    let Ok(compiled) = QueryBuilder::from_inline(query).compile(support::javascript_grammar())
    else {
        return;
    };
    if !compiled.is_valid() {
        return;
    }
    let _ = compiled.emit_types(TypeScriptCodegenConfig::new());
    let _ = compiled.emit(BytecodeConfig::new());
}

fn source_smoke_module() -> &'static Module {
    static MODULE: OnceLock<Module> = OnceLock::new();
    MODULE.get_or_init(|| {
        let compiled = QueryBuilder::from_inline("Q = (_)")
            .compile(support::javascript_grammar())
            .expect("smoke query compiles");
        assert!(
            compiled.is_valid(),
            "{}",
            compiled.diagnostics().render(compiled.source_map())
        );
        compiled
            .emit(BytecodeConfig::new())
            .expect("bytecode emission answers")
            .into_artifact()
            .expect("smoke query emits a module")
    })
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let language: TsLanguage = arborium_javascript::language().into();
    parser
        .set_language(&language)
        .expect("set javascript language");
    parser.parse(source, None).expect("parse source")
}

fn run_source_smoke(source: &str) {
    let module = source_smoke_module();
    let tree = parse_js(source);
    let entry = module.entry_point("Q").expect("Q entry point exists");
    let journal = VM::builder(source, &tree)
        .build()
        .execute(module, &entry)
        .expect("wildcard root matches every parsed tree");
    let value = materialize_verified(
        source,
        module,
        &entry,
        journal.output_events(),
        Colors::new(false),
    );

    assert_eq!(value, Value::Absent);
}

fn arb_query_text() -> impl Strategy<Value = String> {
    prop_oneof![
        select(VALID_SMOKE_QUERIES.to_vec()).prop_map(str::to_string),
        ".{0,200}".prop_map(|text: String| text),
        proptest::collection::vec(
            prop_oneof![
                Just("("),
                Just(")"),
                Just("["),
                Just("]"),
                Just("{"),
                Just("}"),
                Just(" "),
                Just("@x"),
                Just("identifier"),
                Just("Q = "),
                Just(":"),
                Just("*"),
                Just("+"),
                Just("?"),
                Just("_"),
                Just("\""),
                Just("."),
                Just("="),
            ],
            0..40,
        )
        .prop_map(|tokens| tokens.concat()),
    ]
}

fn arb_source_text() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,200}".prop_map(|text: String| text),
        (0usize..400).prop_map(|depth| format!("{}x", "!".repeat(depth))),
        (0usize..400).prop_map(|depth| format!("{}0{}", "(".repeat(depth), ")".repeat(depth))),
        (0usize..400).prop_map(|depth| format!("{}{}", "[".repeat(depth), "]".repeat(depth))),
    ]
}

proptest! {
    #![proptest_config(property_config(256))]

    #[test]
    fn compiler_arbitrary_text_is_total(text in arb_query_text()) {
        try_compile_smoke(&text);
    }

    #[test]
    fn formatter_arbitrary_text_is_total_and_idempotent(text in arb_query_text()) {
        if let Ok(formatted) = format_query(&text) {
            prop_assert_eq!(format_query(&formatted).expect("formatted query reparses"), formatted);
        }
    }

    #[test]
    fn vm_arbitrary_source_is_total(source in arb_source_text()) {
        run_source_smoke(&source);
    }
}
