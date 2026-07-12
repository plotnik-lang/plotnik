//! Conformance: for every runnable 06-vm fixture, the generated Rust module
//! must agree with the bytecode VM at both levels of the contract —
//!
//! - **trace**: the matcher's committed effect stream equals the VM's,
//!   entry for entry;
//! - **value**: the safe typed `parse` output, serialized through the serde
//!   channel, equals the VM's materialized value as JSON (compared as JSON
//!   values — the two sides legitimately order struct fields differently), and
//!   `matches` agrees with the VM's yes/no outcome.
//!
//! Each fixture's query is compiled twice — to a bytecode module (executed
//! here, in-process, as the oracle) and to Rust module source. The VM's
//! committed effects and materialized value are baked into one generated
//! program alongside every module; that program re-parses each input with the
//! real grammars and asserts agreement. `trybuild` builds *and runs* it, so
//! this one target proves both that emitted code compiles and that it behaves
//! like the VM across the corpus. A final hand-rolled module pins the
//! compiled-in limit policy: an `Of(1)` step budget must trip `parse`.
//! The same generated program includes every committed Rust section under
//! `04-emit`, so the full generated-Rust corpus costs one compiler invocation.
//!
//! Inspection and recording fixtures are excluded: spans and step recordings
//! are VM-only diagnostic channels the generated matcher deliberately lacks.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use plotnik_lib::bytecode::{Module, TypeDefKind, TypeKind};
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{
    BytecodeConfig, Colors, QueryBuilder, RuntimeError, RustCodegenConfig, SourceMap, SourcePath,
    VM, matcher_entry_fn_name, materialize_verified,
};
use plotnik_rt::{Limit, RuntimeLimitSpec};
use plotnik_tests::conformance::{Fixture, collect_vm_fixtures, render_effect};
use tree_sitter::{Language as TsLanguage, Parser as TsParser};

#[path = "../test_support/grammar_loader.rs"]
mod grammar_loader;
#[path = "rust_sections_compile.rs"]
mod rust_sections_compile;

#[test]
fn generated_matchers_replay_vm_traces_and_golden_rust_sections_compile() {
    let fixtures = collect_vm_fixtures().expect("discover 06-vm fixtures");

    let mut program = String::from(PRELUDE);
    let mut runs = Vec::new();
    let mut skipped = Vec::new();
    for fx in fixtures.iter().filter(|fixture| fixture.excluded.is_none()) {
        match conformance_mod(fx) {
            Some(text) => {
                program.push('\n');
                program.push_str(&text);
                runs.push(mod_ident(&fx.name));
            }
            None => skipped.push(fx.name.as_str()),
        }
    }
    if !skipped.is_empty() {
        eprintln!(
            "skipped (query has errors or no callable entrypoints): {}",
            skipped.join(", ")
        );
    }
    // A collapse of the corpus (mass skip, broken discovery) must fail loudly
    // rather than shrink coverage in silence.
    assert!(
        runs.len() >= 200,
        "expected the full 06-vm corpus, generated only {} fixture modules",
        runs.len()
    );
    program.push('\n');
    program.push_str(&limit_trip_mod());
    runs.push("limit_trip".to_string());
    program.push('\n');
    program.push_str(&unbounded_mod());
    runs.push("unbounded".to_string());
    program.push('\n');
    program.push_str(&steps_only_mod());
    runs.push("steps_only".to_string());
    let distinct: BTreeSet<&str> = runs.iter().map(String::as_str).collect();
    assert_eq!(distinct.len(), runs.len(), "fixture module names collide");

    rust_sections_compile::append_golden_rust_sections(&mut program);
    program.push_str("\nfn main() {\n");
    for ident in &runs {
        writeln!(program, "    {ident}::run();").expect("writing to a String is infallible");
    }
    writeln!(
        program,
        "    println!(\"{} fixtures conform\");",
        runs.len()
    )
    .expect("writing to a String is infallible");
    program.push_str("}\n");

    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("codegen-conformance");
    fs::create_dir_all(&dir).expect("create trybuild scratch dir");
    let file = dir.join("vm_conformance.rs");
    // Write-if-changed keeps trybuild's rebuild check keyed to real content
    // changes instead of every test invocation.
    if fs::read_to_string(&file).is_ok_and(|old| old == program) {
        eprintln!("conformance program unchanged: {}", file.display());
    } else {
        fs::write(&file, program).expect("write generated program");
    }

    let cases = trybuild::TestCases::new();
    cases.pass(&file);
}

/// One conformance module: the fixture's input, the VM's expected stream and
/// value, the generated module, and a `run()` gluing them together. `None`
/// when the query has compile errors (a diagnostics fixture — nothing to
/// execute).
fn conformance_mod(fx: &Fixture) -> Option<String> {
    let lang = resolve_lang(fx.ext.as_deref());
    let compiled = QueryBuilder::new(source_map(&fx.query))
        .compile(lang.grammar)
        .expect("query parsing should not exhaust fuel");
    if compiled.diagnostics().has_errors() {
        return None;
    }
    let module = compiled
        .emit(BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("a query without errors compiles to a module");
    let entry = module.entrypoint_names().last()?.to_string();
    let expected = vm_expected(&lang, &module, &entry, &fx.input, &fx.name);
    let matcher = compiled
        .emit(RustCodegenConfig::new().serde(true))
        .expect("Rust emission answers")
        .into_artifact()
        .expect("a query without errors compiles to a matcher")
        .into_source();

    let mut out = String::new();
    let w = &mut out;
    writeln!(w, "mod {} {{", mod_ident(&fx.name)).expect("writing to a String is infallible");
    writeln!(w, "    const NAME: &str = {:?};", fx.name)
        .expect("writing to a String is infallible");
    writeln!(w, "    const SOURCE: &str = {:?};", fx.input)
        .expect("writing to a String is infallible");
    match &expected {
        Some(run) => {
            w.push_str("    const EXPECTED: Option<&[&str]> = Some(&[\n");
            for line in &run.effects {
                writeln!(w, "        {line:?},").expect("writing to a String is infallible");
            }
            w.push_str("    ]);\n");
            writeln!(
                w,
                "    const EXPECTED_JSON: Option<&str> = Some({:?});",
                run.json
            )
            .expect("writing to a String is infallible");
        }
        None => {
            w.push_str("    const EXPECTED: Option<&[&str]> = None;\n");
            w.push_str("    const EXPECTED_JSON: Option<&str> = None;\n");
        }
    }
    w.push_str("\n    mod matcher {\n");
    for line in matcher.lines() {
        if line.is_empty() {
            w.push('\n');
        } else {
            writeln!(w, "        {line}").expect("writing to a String is infallible");
        }
    }
    w.push_str("    }\n");
    w.push_str("\n    pub fn run() {\n");
    writeln!(
        w,
        "        let tree = crate::__plotnik_conformance::parse(crate::__plotnik_conformance::Lang::{}, SOURCE);",
        lang.tag
    )
    .expect("writing to a String is infallible");
    writeln!(
        w,
        "        crate::__plotnik_conformance::check(NAME, matcher::{}(&tree, SOURCE), EXPECTED);",
        matcher_entry_fn_name(&entry),
    )
    .expect("writing to a String is infallible");
    value_channel(w, &module, &entry);
    w.push_str("    }\n}\n");
    Some(out)
}

/// The value-level differential inside a fixture's `run()`: call the typed
/// entry point matching the definition's output shape. For parsed values, diff
/// the serialized value against the VM's JSON, and require nominal `matches` to
/// agree with the VM's yes/no outcome. Void entries expose only `matches`.
fn value_channel(w: &mut String, module: &Module, entry: &str) {
    match entry_shape(module, entry) {
        EntryShape::Matches => {
            writeln!(
                w,
                "        let matched = matcher::{entry}::matches(&tree, SOURCE).expect(\"auto limits fit the corpus\");"
            )
            .expect("writing to a String is infallible");
            writeln!(
                w,
                "        assert_eq!(matched, EXPECTED.is_some(), \"{{NAME}}: matches() diverges from the VM outcome\");"
            )
            .expect("writing to a String is infallible");
        }
        EntryShape::Nominal => {
            let parse = format!("{entry}::parse");
            writeln!(
                w,
                "        let parsed = matcher::{parse}(&tree, SOURCE).expect(\"auto limits fit the corpus\");"
            )
            .expect("writing to a String is infallible");
            writeln!(
                w,
                "        let matched = matcher::{entry}::matches(&tree, SOURCE).expect(\"auto limits fit the corpus\");"
            )
            .expect("writing to a String is infallible");
            writeln!(
                w,
                "        assert_eq!(matched, EXPECTED.is_some(), \"{{NAME}}: matches() diverges from the VM outcome\");"
            )
            .expect("writing to a String is infallible");
            w.push_str("        let json = parsed.map(|v| {\n");
            w.push_str(
                "            serde_json::to_string(&plotnik_rt::WithSource::new(&v, SOURCE))\n",
            );
            w.push_str("                .expect(\"typed output serializes\")\n");
            w.push_str("        });\n");
            w.push_str(
                "        crate::__plotnik_conformance::check_value(NAME, json.as_deref(), EXPECTED_JSON);\n",
            );
        }
    }
}

/// How a definition's output surfaces in the generated module. Decided from
/// the bytecode type table — the same partition the emitter draws at the
/// analysis level (struct/enum are nominal items, everything else an alias).
enum EntryShape {
    /// Void output: inherent `{Type}::matches`.
    Matches,
    /// Struct/enum output: inherent `{Type}::parse`.
    Nominal,
}

fn entry_shape(module: &Module, entry: &str) -> EntryShape {
    let entrypoint = module
        .entrypoint(entry)
        .expect("selected definition must be an entrypoint");
    let def = module
        .types()
        .get(entrypoint.result_type())
        .expect("entry result type is in the type table");
    match def.decode() {
        TypeDefKind::Primitive(TypeKind::Void) => EntryShape::Matches,
        TypeDefKind::Struct { .. } | TypeDefKind::Enum { .. } => EntryShape::Nominal,
        TypeDefKind::Primitive(_) | TypeDefKind::Wrapper { .. } => {
            unreachable!("callable definitions must be nominal or void")
        }
    }
}

/// One VM run's observable behavior: the committed effect stream (rendered
/// to comparison lines) and the materialized value as JSON.
struct VmRun {
    effects: Vec<String>,
    json: String,
}

/// The oracle: run the bytecode VM over the fixture input, render its
/// committed effects, and materialize its value. `None` is the no-match
/// outcome; resource-limit errors fail the harness — a fixture that exhausts
/// the VM has no golden behavior to conform to.
fn vm_expected(
    lang: &Lang,
    module: &Module,
    entry: &str,
    source: &str,
    name: &str,
) -> Option<VmRun> {
    let mut parser = TsParser::new();
    parser
        .set_language(&lang.ts)
        .expect("set tree-sitter language");
    let tree = parser.parse(source, None).expect("parse fixture input");
    let entrypoint = module
        .entrypoint(entry)
        .expect("selected definition must be an entrypoint");
    let vm = VM::builder(source, &tree).build();
    match vm.execute(module, &entrypoint) {
        Ok(effects) => {
            let value =
                materialize_verified(source, module, &entrypoint, effects.as_slice(), Colors::OFF);
            VmRun {
                effects: effects.as_slice().iter().map(render_effect).collect(),
                json: serde_json::to_string(&value).expect("VM value serializes"),
            }
            .into()
        }
        Err(RuntimeError::NoMatch) => None,
        Err(err) => panic!("{name}: VM oracle failed: {err}"),
    }
}

/// A module pinning the compiled-in limit policy end to end: with a one-step
/// budget the safe entry points must trip.
fn limit_trip_mod() -> String {
    let query = "Q = (program (expression_statement (identifier) @id))";
    let compiled = QueryBuilder::new(source_map(query))
        .compile(&JS_GRAMMAR)
        .expect("query parsing should not exhaust fuel");
    assert!(
        !compiled.diagnostics().has_errors(),
        "limit-trip query must compile"
    );
    let matcher = compiled
        .emit(RustCodegenConfig::new().limits(RuntimeLimitSpec {
            steps: Limit::Of(1),
            memory: Limit::Auto,
        }))
        .expect("Rust emission answers")
        .into_artifact()
        .expect("limit-trip query compiles to a matcher")
        .into_source();

    let mut out = String::new();
    let w = &mut out;
    w.push_str("mod limit_trip {\n");
    w.push_str("    const SOURCE: &str = \"x;\";\n");
    w.push_str("\n    mod matcher {\n");
    for line in matcher.lines() {
        if line.is_empty() {
            w.push('\n');
        } else {
            writeln!(w, "        {line}").expect("writing to a String is infallible");
        }
    }
    w.push_str("    }\n");
    w.push_str("\n    pub fn run() {\n");
    w.push_str("        let tree = crate::__plotnik_conformance::parse(crate::__plotnik_conformance::Lang::Js, SOURCE);\n");
    w.push_str("        let err = matcher::Q::parse(&tree, SOURCE)\n");
    w.push_str("            .expect_err(\"limit_trip: a one-step budget must trip\");\n");
    w.push_str("        assert_eq!(err, plotnik_rt::LimitExceeded::Steps(1));\n");
    w.push_str("        let err = matcher::Q::matches(&tree, SOURCE)\n");
    w.push_str("            .expect_err(\"limit_trip: a one-step budget must trip\");\n");
    w.push_str("        assert_eq!(err, plotnik_rt::LimitExceeded::Steps(1));\n");
    w.push_str("    }\n}\n");
    out
}

/// A module pinning the fully-unbounded opt-out: with both resources unbounded
/// the safe entry points monomorphize to an unmetered `run` — no `run::<true, …>`
/// survives, and `heap_bytes` is never sampled — yet they still match.
fn unbounded_mod() -> String {
    let query = "Q = (program (expression_statement (identifier) @id))";
    let compiled = QueryBuilder::new(source_map(query))
        .compile(&JS_GRAMMAR)
        .expect("query parsing should not exhaust fuel");
    assert!(
        !compiled.diagnostics().has_errors(),
        "unbounded query must compile"
    );
    let matcher = compiled
        .emit(RustCodegenConfig::new().limits(RuntimeLimitSpec {
            steps: Limit::Unbounded,
            memory: Limit::Unbounded,
        }))
        .expect("Rust emission answers")
        .into_artifact()
        .expect("unbounded query compiles to a matcher")
        .into_source();
    assert!(
        !matcher.contains("run::<true,"),
        "unbounded policy must not instantiate a metered `run`:\n{matcher}"
    );
    assert!(
        matcher.contains("run::<false, false, false>"),
        "unbounded `matches` must run fully unmetered and suppressed:\n{matcher}"
    );

    let mut out = String::new();
    let w = &mut out;
    w.push_str("mod unbounded {\n");
    w.push_str("    const SOURCE: &str = \"x;\";\n");
    w.push_str("\n    mod matcher {\n");
    for line in matcher.lines() {
        if line.is_empty() {
            w.push('\n');
        } else {
            writeln!(w, "        {line}").expect("writing to a String is infallible");
        }
    }
    w.push_str("    }\n");
    w.push_str("\n    pub fn run() {\n");
    w.push_str("        let tree = crate::__plotnik_conformance::parse(crate::__plotnik_conformance::Lang::Js, SOURCE);\n");
    w.push_str("        let parsed = matcher::Q::parse(&tree, SOURCE)\n");
    w.push_str("            .expect(\"unbounded: parse cannot trip a limit\");\n");
    w.push_str("        assert!(parsed.is_some(), \"unbounded: Q matches x;\");\n");
    w.push_str("        let matched = matcher::Q::matches(&tree, SOURCE)\n");
    w.push_str("            .expect(\"unbounded: matches cannot trip a limit\");\n");
    w.push_str("        assert!(matched, \"unbounded: Q matches x;\");\n");
    w.push_str("    }\n}\n");
    out
}

/// A module pinning independent per-resource metering: steps bounded, memory
/// unbounded. The memory check folds out (`run::<true, false, …>`) while the
/// one-step budget still trips.
fn steps_only_mod() -> String {
    let query = "Q = (program (expression_statement (identifier) @id))";
    let compiled = QueryBuilder::new(source_map(query))
        .compile(&JS_GRAMMAR)
        .expect("query parsing should not exhaust fuel");
    assert!(
        !compiled.diagnostics().has_errors(),
        "steps-only query must compile"
    );
    let matcher = compiled
        .emit(RustCodegenConfig::new().limits(RuntimeLimitSpec {
            steps: Limit::Of(1),
            memory: Limit::Unbounded,
        }))
        .expect("Rust emission answers")
        .into_artifact()
        .expect("steps-only query compiles to a matcher")
        .into_source();
    assert!(
        matcher.contains("run::<true, false,"),
        "steps-bounded, memory-unbounded must meter steps only:\n{matcher}"
    );

    let mut out = String::new();
    let w = &mut out;
    w.push_str("mod steps_only {\n");
    w.push_str("    const SOURCE: &str = \"x;\";\n");
    w.push_str("\n    mod matcher {\n");
    for line in matcher.lines() {
        if line.is_empty() {
            w.push('\n');
        } else {
            writeln!(w, "        {line}").expect("writing to a String is infallible");
        }
    }
    w.push_str("    }\n");
    w.push_str("\n    pub fn run() {\n");
    w.push_str("        let tree = crate::__plotnik_conformance::parse(crate::__plotnik_conformance::Lang::Js, SOURCE);\n");
    w.push_str("        let err = matcher::Q::parse(&tree, SOURCE)\n");
    w.push_str("            .expect_err(\"steps_only: a one-step budget must trip\");\n");
    w.push_str("        assert_eq!(err, plotnik_rt::LimitExceeded::Steps(1));\n");
    w.push_str("    }\n}\n");
    out
}

const PRELUDE: &str = r#"//! Conformance program generated by `codegen_conformance.rs` — do not edit.
//! Each `fx_*` module holds one 06-vm fixture: its input, the VM's committed
//! effect stream (rendered by the harness), and the generated matcher.

#![allow(dead_code)]

mod __plotnik_conformance {
use plotnik_rt as rt;

pub(crate) enum Lang {
    Js,
    Ts,
    Dart,
}

pub(crate) fn parse(lang: Lang, source: &str) -> rt::Tree {
    let language: tree_sitter::Language = match lang {
        Lang::Js => arborium_javascript::language().into(),
        Lang::Ts => arborium_typescript::language().into(),
        Lang::Dart => arborium_dart::language().into(),
    };
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language)
        .expect("set tree-sitter language");
    parser.parse(source, None).expect("parse fixture input")
}

fn render_effect(effect: &rt::RuntimeEffect<'_>) -> String {
    plotnik_tests::conformance::render_effect(effect)
}

pub(crate) fn check(name: &str, got: Option<rt::EffectLog<'_>>, expected: Option<&[&str]>) {
    match (got, expected) {
        (None, None) => {}
        (Some(log), Some(lines)) => {
            let got: Vec<String> = log.as_slice().iter().map(render_effect).collect();
            assert_eq!(
                got, lines,
                "{name}: generated matcher diverges from the VM effect stream"
            );
        }
        (got, expected) => panic!(
            "{name}: outcome diverges — generated matcher: {}, VM: {}",
            outcome(got.is_some()),
            outcome(expected.is_some()),
        ),
    }
}

/// Compare serialized typed output against the VM's materialized value as
/// JSON values, not strings: the VM orders struct fields by effect-firing
/// order, generated serde impls by declaration order, and both are correct.
pub(crate) fn check_value(name: &str, got: Option<&str>, expected: Option<&str>) {
    match (got, expected) {
        (None, None) => {}
        (Some(got), Some(expected)) => {
            let got: serde_json::Value =
                serde_json::from_str(got).expect("typed output serializes to JSON");
            let expected: serde_json::Value =
                serde_json::from_str(expected).expect("VM value serializes to JSON");
            assert_eq!(
                got, expected,
                "{name}: typed output diverges from the VM value"
            );
        }
        (got, expected) => panic!(
            "{name}: value outcome diverges — typed parse: {}, VM: {}",
            outcome(got.is_some()),
            outcome(expected.is_some()),
        ),
    }
}

fn outcome(matched: bool) -> &'static str {
    if matched { "match" } else { "no match" }
}
}
"#;

/// `06-vm/captures/single_node` → `fx_06_vm_captures_single_node`.
fn mod_ident(name: &str) -> String {
    let mut out = String::from("fx_");
    out.extend(
        name.chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' }),
    );
    out
}

struct Lang {
    grammar: &'static Grammar,
    ts: TsLanguage,
    /// Variant name in the generated program's `Lang` enum.
    tag: &'static str,
}

fn resolve_lang(ext: Option<&str>) -> Lang {
    match ext {
        None | Some("js" | "javascript" | "jsx") => Lang {
            grammar: &JS_GRAMMAR,
            ts: arborium_javascript::language().into(),
            tag: "Js",
        },
        Some("ts" | "typescript") => Lang {
            grammar: &TS_GRAMMAR,
            ts: arborium_typescript::language().into(),
            tag: "Ts",
        },
        Some("dart") => Lang {
            grammar: &DART_GRAMMAR,
            ts: arborium_dart::language().into(),
            tag: "Dart",
        },
        Some(other) => panic!("input language `{other}` is not wired into the conformance suite"),
    }
}

static JS_GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| load_grammar("arborium-javascript"));
static TS_GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| load_grammar("arborium-typescript"));
static DART_GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| load_grammar("arborium-dart"));

fn load_grammar(package: &str) -> Grammar {
    let raw = RawGrammar::from_json(grammar_loader::load_arborium_grammar_json(package))
        .unwrap_or_else(|e| panic!("{package} grammar fixture: {e:?}"));
    Grammar::from_raw(&raw).unwrap_or_else(|e| panic!("{package} grammar metadata: {e:?}"))
}

fn source_map(query: &str) -> SourceMap {
    let mut sm = SourceMap::new();
    sm.add_file(SourcePath::new("query.ptk"), query);
    sm
}
