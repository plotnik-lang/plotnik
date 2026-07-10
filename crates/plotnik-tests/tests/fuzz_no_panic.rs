//! Property safety net for the untrusted-input thesis: *no input crashes the
//! pipeline.* Two invariants, both "returns a value, never panics or aborts":
//!
//!   1. Compiling arbitrary `.ptk` text yields `Ok`/`Err`, never a panic — the
//!      "outside the trust boundary" rule for the compiler front end.
//!   2. Running a compiled query against arbitrary source — then materializing
//!      and formatting the result — never panics or overflows the native stack.
//!
//! The query side of (2) is covered by a fixed set of diverse templates
//! (recursive, alternation, quantifier, fields, suppressive capture) rather than
//! fuzzed query text,
//! because randomly generated queries almost never compile; the *source* is
//! fuzzed, including deep nests that exercise the iterative backtrack/output
//! paths. Runs use the default (auto) limits, so every case terminates.
//!
//! This guards against the *next* recursion or unbounded-heap hole, not just the
//! ones already fixed.

use indoc::indoc;
use proptest::prelude::*;
use proptest::sample::select;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_lib::bytecode::Module;
use plotnik_lib::{
    BytecodeConfig, Colors, QueryBuilder, TypeScriptCodegenConfig, VM, materialize_verified,
};

mod support;

/// Queries known to compile, spanning the shapes whose runtime paths matter:
/// a root match, a leaf match, an alternation, a scalar quantifier, a row
/// quantifier, a self-recursive definition, and a recursive suppressive capture
/// (the `@_` SuppressBegin/skip/SuppressEnd path).
const TEMPLATES: &[&str] = &[
    "Q = (program) @p",
    "Q = (identifier) @id",
    "Q = [(identifier) @a (number) @b]",
    "Q = (program (_)* @items)",
    "Q = (program (expression_statement (_) @stmt)* @rows)",
    indoc!(
        "
        Rec = [Leaf: (statement_block) Deep: (unary_expression (Rec))]
        Top = (program (Rec))
    "
    ),
    indoc!(
        "
        Sup = [Deep: (unary_expression argument: (Sup) @_) Leaf: (identifier) @_]
        SupTop = (program (expression_statement (Sup)))
    "
    ),
];

/// Compile a query without ever panicking: `None` if it does not parse, link
/// cleanly, or emit. Mirrors the production `run` path (validity gate + `emit`).
fn try_compile(query: &str) -> Option<Vec<u8>> {
    let compiled = QueryBuilder::from_inline(query)
        .compile(support::javascript_grammar())
        .ok()?;
    if !compiled.is_valid() {
        return None;
    }
    let _types = compiled
        .emit_types(TypeScriptCodegenConfig::new())
        .ok()?
        .into_artifact()?;
    compiled
        .emit(BytecodeConfig::new())
        .ok()?
        .into_artifact()
        .map(|module| module.bytes().to_vec())
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse source")
}

/// Run the full untrusted pipeline for every entrypoint: execute (auto limits),
/// and on a match materialize + format + drop the value. Any panic or overflow
/// here fails the property.
fn exercise_pipeline(module: &Module, source: &str) {
    let tree = parse_js(source);
    for i in 0..module.entrypoint_count() {
        let entry = module
            .entrypoint_at(i)
            .expect("entrypoint_count bounds entrypoint_at");
        let vm = VM::builder(source, &tree).build();
        if let Ok(effects) = vm.execute(module, &entry) {
            let value = materialize_verified(
                source,
                module,
                &entry,
                effects.as_slice(),
                Colors::new(false),
            );
            let _ = value.format(false, Colors::new(false));
        }
    }
}

/// Source shapes: arbitrary text, deep unary/paren/bracket nests (to drive the
/// iterative backtrack and output paths), and JS-ish token soup.
fn arb_source() -> impl Strategy<Value = String> {
    prop_oneof![
        ".{0,200}".prop_map(|s: String| s),
        (0usize..400).prop_map(|n| format!("{}x", "!".repeat(n))),
        (0usize..400).prop_map(|n| format!("{}0{}", "(".repeat(n), ")".repeat(n))),
        (0usize..400).prop_map(|n| format!("{}{}", "[".repeat(n), "]".repeat(n))),
        proptest::collection::vec(
            prop_oneof![
                Just("let "),
                Just("x"),
                Just("="),
                Just("1"),
                Just("{"),
                Just("}"),
                Just("("),
                Just(")"),
                Just(";"),
                Just("function "),
                Just("return "),
            ],
            0..40,
        )
        .prop_map(|toks| toks.concat()),
    ]
}

/// Query text: valid templates (so some inputs reach the happy path) plus
/// arbitrary text and plotnik token soup (so most stress the error paths).
fn arb_query_text() -> impl Strategy<Value = String> {
    prop_oneof![
        select(TEMPLATES.to_vec()).prop_map(|s| s.to_string()),
        ".{0,200}".prop_map(|s: String| s),
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
        .prop_map(|toks| toks.concat()),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// Compiling untrusted query text never panics — only `Ok`/`Err`.
    #[test]
    fn compiling_arbitrary_text_never_panics(text in arb_query_text()) {
        let _ = try_compile(&text);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// Running a compiled query against arbitrary source — and rendering its
    /// output — never panics or overflows the stack.
    #[test]
    fn run_pipeline_never_panics(template in select(TEMPLATES.to_vec()), source in arb_source()) {
        if let Some(bytes) = try_compile(template) {
            let module = Module::load(&bytes).expect("compiler emits a loadable module");
            exercise_pipeline(&module, &source);
        }
    }
}
