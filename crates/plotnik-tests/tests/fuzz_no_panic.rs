//! Property contracts for Plotnik's untrusted and validated pipeline boundaries.
//!
//! Arbitrary query and source text get bounded smoke passes. Valid query/source
//! pairs use witnessed cases instead: they must compile, emit types and bytecode,
//! retain their named entry point after formatting, and produce one exact runtime
//! outcome. This keeps compiler rejection, universal `NoMatch`, resource
//! exhaustion, and skipped materialization from satisfying the end-to-end
//! properties vacuously.

use std::sync::OnceLock;

use indoc::indoc;
use proptest::prelude::*;
use proptest::sample::select;
use proptest::test_runner::FileFailurePersistence;
use serde::Deserialize;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_lib::bytecode::Module;
use plotnik_lib::{
    BytecodeConfig, Colors, NodeValue, QueryBuilder, RuntimeError, TypeScriptCodegenConfig, VM,
    Value, format_query, materialize_verified,
};

mod support;

const ENTRY: &str = "Q";
const ROOT_QUERY: &str = "Q=(program)@root";
const CHILD_LIST_QUERY: &str = "Q=(program(expression_statement(_)@value)+@items)";
const LABELED_ALTERNATION_QUERY: &str =
    "Q=(program(expression_statement[A:(identifier)@a B:(number)@b]@choice))";
const PREDICATE_QUERY: &str =
    "Q=(program(lexical_declaration(variable_declarator name:(identifier==\"v0\")@id)))";
const SOURCE_SMOKE_QUERY: &str = "Q=(_)";

const RECURSIVE_CAPTURE_QUERY: &str = indoc! {"
    Rec = [
      Leaf: (identifier) @leaf
      Deep: (unary_expression argument: (Rec) @inner)
    ]
    Q = (program (expression_statement (Rec) @root))
"};

const RECURSIVE_DISCARD_QUERY: &str = indoc! {"
    Nested = (call_expression function: [
      (identifier) @name
      (Nested) @inner
    ])
    Q = (program
      (expression_statement (identifier) @kept)
      (expression_statement (Nested) @_)
    )
"};

const VALID_SMOKE_QUERIES: &[&str] = &[
    ROOT_QUERY,
    CHILD_LIST_QUERY,
    LABELED_ALTERNATION_QUERY,
    RECURSIVE_CAPTURE_QUERY,
    RECURSIVE_DISCARD_QUERY,
    PREDICATE_QUERY,
    SOURCE_SMOKE_QUERY,
];

#[derive(Debug)]
struct Case {
    scenario: &'static str,
    query: String,
    source: String,
    expected: Expected,
}

#[derive(Debug)]
enum Expected {
    Match(ValueSpec),
    NoMatch,
}

#[derive(Debug)]
enum ValueSpec {
    Node {
        kind: &'static str,
        start: usize,
        end: usize,
    },
    List(Vec<ValueSpec>),
    Record(Vec<(&'static str, ValueSpec)>),
    Variant {
        case: &'static str,
        payload: Box<ValueSpec>,
    },
}

impl ValueSpec {
    fn node(kind: &'static str, start: usize, end: usize) -> Self {
        Self::Node { kind, start, end }
    }

    fn list(items: Vec<ValueSpec>) -> Self {
        Self::List(items)
    }

    fn record<const N: usize>(fields: [(&'static str, ValueSpec); N]) -> Self {
        Self::Record(Vec::from(fields))
    }

    fn variant<const N: usize>(case: &'static str, fields: [(&'static str, ValueSpec); N]) -> Self {
        Self::Variant {
            case,
            payload: Box::new(Self::record(fields)),
        }
    }

    fn materialize<'s>(&self, source: &'s str) -> Value<'s> {
        match self {
            Self::Node { kind, start, end } => {
                let text = source.get(*start..*end).unwrap_or_else(|| {
                    panic!("invalid expected node span {start}..{end} for {source:?}")
                });
                Value::Node(NodeValue {
                    kind,
                    text,
                    span: (
                        (*start).try_into().expect("generated span fits in u32"),
                        (*end).try_into().expect("generated span fits in u32"),
                    ),
                })
            }
            Self::List(items) => {
                Value::List(items.iter().map(|item| item.materialize(source)).collect())
            }
            Self::Record(fields) => Value::Record(
                fields
                    .iter()
                    .map(|(name, value)| (*name, value.materialize(source)))
                    .collect(),
            ),
            Self::Variant { case, payload } => Value::Variant {
                case,
                payload: Some(Box::new(payload.materialize(source))),
            },
        }
    }
}

#[derive(Debug)]
struct NoMatchCase {
    positive_control: Case,
    negative: Case,
}

struct CompiledArtifacts {
    module: Module,
    typescript: String,
}

#[derive(Clone, Debug)]
enum Statement {
    Identifier(u16),
    Number(u16),
    Unary { depth: usize, name: u16 },
    Lexical { name: u16, value: u16 },
    Debugger,
}

impl Statement {
    fn render(&self) -> String {
        match self {
            Self::Identifier(name) => format!("v{name};"),
            Self::Number(value) => format!("{value};"),
            Self::Unary { depth, name } => format!("{}v{name};", "!".repeat(*depth)),
            Self::Lexical { name, value } => format!("let v{name} = {value};"),
            Self::Debugger => "debugger;".to_string(),
        }
    }

    fn expression_kind(&self) -> &'static str {
        match self {
            Self::Identifier(_) => "identifier",
            Self::Number(_) => "number",
            Self::Unary { .. } => "unary_expression",
            Self::Lexical { .. } | Self::Debugger => {
                panic!("only expression statements have an expression kind")
            }
        }
    }
}

#[derive(Clone, Debug)]
struct Program(Vec<Statement>);

impl Program {
    fn render(&self) -> String {
        self.0
            .iter()
            .map(Statement::render)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn child_record_specs(&self) -> Vec<ValueSpec> {
        let mut offset = 0usize;
        self.0
            .iter()
            .enumerate()
            .map(|(index, statement)| {
                if index > 0 {
                    offset += 1;
                }
                let text = statement.render();
                let start = offset;
                offset += text.len();
                ValueSpec::record([(
                    "value",
                    ValueSpec::node(statement.expression_kind(), start, offset - 1),
                )])
            })
            .collect()
    }
}

fn property_config(cases: u32) -> ProptestConfig {
    ProptestConfig {
        cases,
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource(
            "proptest-regressions",
        ))),
        ..ProptestConfig::default()
    }
}

/// Compilation is deliberately permissive here: arbitrary text may fail at any
/// validated boundary. Panics remain test failures.
fn try_compile_smoke(query: &str) -> Option<Module> {
    let compiled = QueryBuilder::from_inline(query)
        .compile(support::javascript_grammar())
        .ok()?;
    if !compiled.is_valid() {
        return None;
    }
    compiled
        .emit_types(TypeScriptCodegenConfig::new())
        .ok()?
        .into_artifact()?;
    compiled.emit(BytecodeConfig::new()).ok()?.into_artifact()
}

fn compile_valid(query: &str, scenario: &str) -> CompiledArtifacts {
    let compiled = QueryBuilder::from_inline(query)
        .compile(support::javascript_grammar())
        .unwrap_or_else(|error| panic!("{scenario}: generated query could not compile: {error}"));
    assert!(
        compiled.is_valid(),
        "{scenario}: generated query is invalid:\n{}",
        compiled.diagnostics().render(compiled.source_map())
    );

    let typescript = compiled
        .emit_types(TypeScriptCodegenConfig::new())
        .unwrap_or_else(|error| panic!("{scenario}: TypeScript emission failed: {error}"))
        .into_artifact()
        .unwrap_or_else(|| panic!("{scenario}: TypeScript emission produced no artifact"))
        .into_parts()
        .0;
    let module = compiled
        .emit(BytecodeConfig::new())
        .unwrap_or_else(|error| panic!("{scenario}: bytecode emission failed: {error}"))
        .into_artifact()
        .unwrap_or_else(|| panic!("{scenario}: bytecode emission produced no module"));
    assert!(
        module.entry_point_count() > 0,
        "{scenario}: emitted module has no entry points"
    );
    assert!(
        module.entry_point(ENTRY).is_some(),
        "{scenario}: emitted module is missing entry point `{ENTRY}`"
    );

    CompiledArtifacts { module, typescript }
}

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse source")
}

fn execute_on_tree<'s>(
    module: &'s Module,
    source: &'s str,
    tree: &Tree,
    scenario: &str,
) -> Result<Value<'s>, RuntimeError> {
    let entry = module
        .entry_point(ENTRY)
        .unwrap_or_else(|| panic!("{scenario}: missing entry point `{ENTRY}`"));
    let vm = VM::builder(source, tree).build();
    let journal = vm.execute(module, &entry)?;
    let value = materialize_verified(
        source,
        module,
        &entry,
        journal.output_events(),
        Colors::new(false),
    );

    Ok(value)
}

fn execute<'s>(
    module: &'s Module,
    source: &'s str,
    scenario: &str,
) -> Result<Value<'s>, RuntimeError> {
    let tree = parse_js(source);
    assert!(
        !tree.root_node().has_error(),
        "{scenario}: generated invalid JavaScript: {source:?}"
    );
    execute_on_tree(module, source, &tree, scenario)
}

fn parse_json(json: &str, scenario: &str) -> serde_json::Value {
    let mut deserializer = serde_json::Deserializer::from_str(json);
    deserializer.disable_recursion_limit();
    let value = serde_json::Value::deserialize(&mut deserializer)
        .unwrap_or_else(|error| panic!("{scenario}: result is not valid JSON: {error}"));
    deserializer
        .end()
        .unwrap_or_else(|error| panic!("{scenario}: result has trailing JSON input: {error}"));
    value
}

fn assert_expected(
    scenario: &str,
    expected: &Expected,
    actual: Result<Value<'_>, RuntimeError>,
    source: &str,
    query_form: &str,
) {
    match (expected, actual) {
        (Expected::Match(expected), Ok(actual)) => {
            let expected = expected.materialize(source);
            assert_eq!(
                &expected, &actual,
                "{scenario}: wrong result from {query_form} query"
            );

            let expected_json = serde_json::to_string(&expected).unwrap_or_else(|error| {
                panic!("{scenario}: expected value is not serializable: {error}")
            });
            let rendered = actual.format(false, Colors::new(false));
            assert_eq!(
                parse_json(&expected_json, scenario),
                parse_json(&rendered, scenario),
                "{scenario}: runtime formatter changed the {query_form} result"
            );
        }
        (Expected::NoMatch, Err(RuntimeError::NoMatch)) => {}
        (Expected::Match(_), Err(error)) => {
            panic!("{scenario}: {query_form} query should match, got {error}")
        }
        (Expected::NoMatch, Ok(actual)) => {
            panic!("{scenario}: {query_form} query should not match, got {actual:?}")
        }
        (Expected::NoMatch, Err(error)) => {
            panic!("{scenario}: {query_form} query should return NoMatch, got {error}")
        }
    }
}

fn assert_case(case: Case) {
    let formatted = format_query(&case.query).unwrap_or_else(|error| {
        panic!("{}: generated query did not format: {error}", case.scenario)
    });
    let formatted_twice = format_query(&formatted).unwrap_or_else(|error| {
        panic!(
            "{}: formatted query did not format again: {error}",
            case.scenario
        )
    });
    assert_eq!(
        formatted, formatted_twice,
        "{}: formatter is not idempotent",
        case.scenario
    );

    let original = compile_valid(&case.query, case.scenario);
    let canonical = compile_valid(&formatted, case.scenario);
    assert_eq!(
        original.typescript, canonical.typescript,
        "{}: formatting changed the inferred result types",
        case.scenario
    );

    let original_result = execute(&original.module, &case.source, case.scenario);
    assert_expected(
        case.scenario,
        &case.expected,
        original_result,
        &case.source,
        "original",
    );
    let canonical_result = execute(&canonical.module, &case.source, case.scenario);
    assert_expected(
        case.scenario,
        &case.expected,
        canonical_result,
        &case.source,
        "formatted",
    );
}

fn source_smoke_module() -> &'static Module {
    static MODULE: OnceLock<Module> = OnceLock::new();
    MODULE.get_or_init(|| compile_valid(SOURCE_SMOKE_QUERY, "arbitrary source smoke").module)
}

fn assert_arbitrary_source_smoke(source: &str) {
    let tree = parse_js(source);
    let actual = execute_on_tree(
        source_smoke_module(),
        source,
        &tree,
        "arbitrary source smoke",
    )
    .unwrap_or_else(|error| {
        panic!(
            "wildcard root query failed on arbitrary source: {error}; tree: {}",
            tree.root_node().to_sexp()
        )
    });

    assert_eq!(Value::Absent, actual);
}

fn arb_statement() -> impl Strategy<Value = Statement> {
    prop_oneof![
        any::<u16>().prop_map(Statement::Identifier),
        any::<u16>().prop_map(Statement::Number),
        (1usize..=8, any::<u16>()).prop_map(|(depth, name)| Statement::Unary { depth, name }),
        (any::<u16>(), any::<u16>()).prop_map(|(name, value)| Statement::Lexical { name, value }),
        Just(Statement::Debugger),
    ]
}

fn arb_program() -> impl Strategy<Value = Program> {
    proptest::collection::vec(arb_statement(), 1..=8).prop_map(Program)
}

fn arb_expression_program() -> impl Strategy<Value = Program> {
    let statement = prop_oneof![
        any::<u16>().prop_map(Statement::Identifier),
        any::<u16>().prop_map(Statement::Number),
        (1usize..=8, any::<u16>()).prop_map(|(depth, name)| Statement::Unary { depth, name }),
    ];
    proptest::collection::vec(statement, 2..=8).prop_map(Program)
}

fn arb_root_case() -> impl Strategy<Value = Case> {
    arb_program().prop_map(|program| {
        let source = program.render();
        let end = source.len();
        Case {
            scenario: "root capture",
            query: ROOT_QUERY.to_string(),
            source: source.clone(),
            expected: Expected::Match(ValueSpec::record([(
                "root",
                ValueSpec::node("program", 0, end),
            )])),
        }
    })
}

fn arb_child_list_case() -> impl Strategy<Value = Case> {
    arb_expression_program().prop_map(|program| Case {
        scenario: "program child list",
        query: CHILD_LIST_QUERY.to_string(),
        source: program.render(),
        expected: Expected::Match(ValueSpec::record([(
            "items",
            ValueSpec::list(program.child_record_specs()),
        )])),
    })
}

fn arb_identifier_alternation_case() -> impl Strategy<Value = Case> {
    any::<u16>().prop_map(|name| {
        let source = format!("λ{name}");
        let end = source.len();
        Case {
            scenario: "labeled alternation identifier branch",
            query: LABELED_ALTERNATION_QUERY.to_string(),
            source,
            expected: Expected::Match(ValueSpec::record([(
                "choice",
                ValueSpec::variant("A", [("a", ValueSpec::node("identifier", 0, end))]),
            )])),
        }
    })
}

fn arb_number_alternation_case() -> impl Strategy<Value = Case> {
    any::<u16>().prop_map(|number| {
        let source = number.to_string();
        let end = source.len();
        Case {
            scenario: "labeled alternation number branch",
            query: LABELED_ALTERNATION_QUERY.to_string(),
            source,
            expected: Expected::Match(ValueSpec::record([(
                "choice",
                ValueSpec::variant("B", [("b", ValueSpec::node("number", 0, end))]),
            )])),
        }
    })
}

fn arb_recursive_capture_case() -> impl Strategy<Value = Case> {
    (2usize..=64, any::<u16>()).prop_map(|(depth, name)| {
        let identifier = format!("v{name}");
        let source = format!("{}{identifier}", "!".repeat(depth));
        let mut result = ValueSpec::variant(
            "Leaf",
            [("leaf", ValueSpec::node("identifier", depth, source.len()))],
        );
        for _ in 0..depth {
            result = ValueSpec::variant("Deep", [("inner", result)]);
        }
        Case {
            scenario: "captured recursion",
            query: RECURSIVE_CAPTURE_QUERY.to_string(),
            source,
            expected: Expected::Match(ValueSpec::record([("root", result)])),
        }
    })
}

fn arb_recursive_discard_case() -> impl Strategy<Value = Case> {
    (2usize..=64).prop_map(|depth| {
        let source = format!("keep; hidden{};", "()".repeat(depth));
        Case {
            scenario: "recursive discard",
            query: RECURSIVE_DISCARD_QUERY.to_string(),
            source,
            expected: Expected::Match(ValueSpec::record([(
                "kept",
                ValueSpec::node("identifier", 0, "keep".len()),
            )])),
        }
    })
}

fn predicate_query(expected_name: &str) -> String {
    format!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier == \"{expected_name}\") @id)))"
    )
}

fn arb_no_match_case() -> impl Strategy<Value = NoMatchCase> {
    any::<u16>().prop_map(|name| {
        let declared = format!("v{name}");
        let other = format!("{declared}_other");
        let source = format!("const {declared} = {other};");
        let start = "const ".len();
        let end = start + declared.len();
        NoMatchCase {
            positive_control: Case {
                scenario: "field predicate positive control",
                query: predicate_query(&declared),
                source: source.clone(),
                expected: Expected::Match(ValueSpec::record([(
                    "id",
                    ValueSpec::node("identifier", start, end),
                )])),
            },
            negative: Case {
                scenario: "field predicate deliberate no-match",
                query: predicate_query(&other),
                source,
                expected: Expected::NoMatch,
            },
        }
    })
}

/// Query text for smoke fuzzing: known-valid pipeline inputs plus arbitrary text
/// and token soup biased toward parser recovery.
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
        (0usize..400).prop_map(|depth| { format!("{}0{}", "(".repeat(depth), ")".repeat(depth)) }),
        (0usize..400).prop_map(|depth| format!("{}{}", "[".repeat(depth), "]".repeat(depth))),
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
        .prop_map(|tokens| tokens.concat()),
    ]
}

proptest! {
    #![proptest_config(property_config(256))]

    #[test]
    fn compiler_arbitrary_text_is_total_smoke(text in arb_query_text()) {
        let _ = try_compile_smoke(&text);
    }

    #[test]
    fn formatter_arbitrary_text_is_total_smoke(text in arb_query_text()) {
        let _ = format_query(&text);
    }

    #[test]
    fn vm_arbitrary_source_is_total_smoke(source in arb_source_text()) {
        assert_arbitrary_source_smoke(&source);
    }
}

proptest! {
    #![proptest_config(property_config(64))]

    #[test]
    fn root_capture_matches_witness(case in arb_root_case()) {
        assert_case(case);
    }

    #[test]
    fn child_list_matches_witness(case in arb_child_list_case()) {
        assert_case(case);
    }

    #[test]
    fn labeled_alternation_identifier_matches_witness(case in arb_identifier_alternation_case()) {
        assert_case(case);
    }

    #[test]
    fn labeled_alternation_number_matches_witness(case in arb_number_alternation_case()) {
        assert_case(case);
    }

    #[test]
    fn recursive_capture_matches_witness(case in arb_recursive_capture_case()) {
        assert_case(case);
    }

    #[test]
    fn recursive_discard_matches_witness(case in arb_recursive_discard_case()) {
        assert_case(case);
    }

    #[test]
    fn field_predicate_is_exact_no_match(case in arb_no_match_case()) {
        assert_case(case.positive_control);
        assert_case(case.negative);
    }
}

#[test]
fn witnessed_boundaries_are_explicit() {
    assert_case(Case {
        scenario: "empty root capture",
        query: ROOT_QUERY.to_string(),
        source: String::new(),
        expected: Expected::Match(ValueSpec::record([(
            "root",
            ValueSpec::node("program", 0, 0),
        )])),
    });
    assert_case(Case {
        scenario: "empty plus repetition",
        query: CHILD_LIST_QUERY.to_string(),
        source: String::new(),
        expected: Expected::NoMatch,
    });
    assert_case(Case {
        scenario: "singleton record list",
        query: CHILD_LIST_QUERY.to_string(),
        source: "v0;".to_string(),
        expected: Expected::Match(ValueSpec::record([(
            "items",
            ValueSpec::list(vec![ValueSpec::record([(
                "value",
                ValueSpec::node("identifier", 0, 2),
            )])]),
        )])),
    });

    let leaf = ValueSpec::variant("Leaf", [("leaf", ValueSpec::node("identifier", 0, 2))]);
    assert_case(Case {
        scenario: "recursive base case",
        query: RECURSIVE_CAPTURE_QUERY.to_string(),
        source: "v0".to_string(),
        expected: Expected::Match(ValueSpec::record([("root", leaf)])),
    });

    let leaf = ValueSpec::variant("Leaf", [("leaf", ValueSpec::node("identifier", 1, 3))]);
    assert_case(Case {
        scenario: "single recursive step",
        query: RECURSIVE_CAPTURE_QUERY.to_string(),
        source: "!v0".to_string(),
        expected: Expected::Match(ValueSpec::record([(
            "root",
            ValueSpec::variant("Deep", [("inner", leaf)]),
        )])),
    });
    assert_case(Case {
        scenario: "recursive discard base case",
        query: RECURSIVE_DISCARD_QUERY.to_string(),
        source: "keep; hidden();".to_string(),
        expected: Expected::Match(ValueSpec::record([(
            "kept",
            ValueSpec::node("identifier", 0, 4),
        )])),
    });
}

#[test]
fn formatting_commented_patterns_is_idempotent() {
    for count in 0..=80 {
        let items = (0..count)
            .map(|index| format!("/* comment {index} */ (leaf)"))
            .collect::<Vec<_>>()
            .join(" ");
        let query = format!("Q = (root {items})");
        let output = format_query(&query).expect("generated query is parse-clean");

        assert_eq!(
            format_query(&output).expect("formatted query reparses"),
            output
        );
    }
}

#[test]
fn formatting_nested_group_with_literal_closer_returns_error() {
    let error = format_query("((\"(\"").expect_err("unclosed nested group must be rejected");

    assert!(error.diagnostics().is_some());
}
