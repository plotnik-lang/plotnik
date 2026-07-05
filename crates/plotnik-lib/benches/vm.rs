//! VM hot-path benchmarks over a deterministic synthetic JavaScript corpus.
//!
//! Every iteration measures the user-visible query cost minus parsing: build a
//! VM, execute the compiled module against a pre-parsed tree, and materialize
//! the result. `parse` benches tree-sitter parsing of the same corpus for
//! scale, and `scan_rows_execute` skips materialization to split VM time from
//! output building.
//!
//! Scenarios, each isolating a different runtime path:
//! - `scan_rows`: enum rows over every top-level statement — sibling
//!   navigation, alternation checkpoints, and effect logging in bulk.
//! - `fn_params`: nested field constraints plus an inner list per row.
//! - `deep_calls`: self-recursive definition descending nested call chains —
//!   Call/Return frames and call-retry checkpoints.
//! - `pred_eq` / `pred_regex`: string and regex predicates that mostly fail —
//!   per-candidate navigation plus `utf8_text` extraction.
//! - `backtrack_storm`: greedy any-star that never finds its tail, capped by
//!   an explicit step budget — pure dispatch + backtracking, zero output.
//!
//! Run: `make bench` (or `make bench FILTER=scan_rows`).
//! Save/compare: `cargo bench -p plotnik-lib --bench vm -- --save-baseline
//! <name>`, then `critcmp <a> <b>`.
//! Profile: `samply record cargo bench -p plotnik-lib --bench vm --
//! --profile-time 15 scan_rows` (bench profile keeps line tables).

use std::hint::black_box;
use std::sync::LazyLock;

use arborium_tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use indoc::indoc;

use plotnik_lib::bytecode::Module;
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::{Colors, Limit, QueryBuilder, RuntimeLimitSpec, VM, materialize_verified};

#[path = "../test_support/grammar_loader.rs"]
mod grammar_loader;

/// Deterministic synthetic JavaScript: five statement templates cycled by
/// block index, so every run on every machine benches identical trees. No
/// randomness — reproducibility beats realism drift.
mod corpus {
    pub fn generate(target_bytes: usize) -> String {
        let mut out = String::with_capacity(target_bytes + 512);
        let mut i = 0usize;
        while out.len() < target_bytes {
            emit_block(&mut out, i);
            i += 1;
        }
        out
    }

    fn emit_block(out: &mut String, i: usize) {
        let m = i % 100;
        let block = match i % 5 {
            0 => format!(
                r#"function handler_{i}(req, res) {{
  const id_{i} = req.params.id;
  let total = 0;
  if (id_{i} > {m}) {{
    total = compute(id_{i}, {m}) + lookup(id_{i});
  }} else {{
    total = fallback(id_{i});
  }}
  return render(total, "case_{i}");
}}
"#
            ),
            1 => format!(
                r#"class Widget{i} {{
  constructor(name) {{
    this.name = name;
    this.slots = [];
  }}
  update_{i}(delta) {{
    return this.slots.push(delta * {m});
  }}
}}
"#
            ),
            2 => {
                let depth = 3 + i % 10;
                let mut expr = format!("leaf(x, {i})");
                for d in 0..depth {
                    expr = format!("step{d}({expr})");
                }
                format!("const chain_{i} = (x) => {expr};\n")
            }
            3 => {
                // A "needle" string every 43rd block index keeps predicate
                // scenarios mostly-failing with a handful of real hits.
                let s = if i.is_multiple_of(43) {
                    "needle".to_owned()
                } else {
                    format!("filler_{i}")
                };
                format!("process(alpha_{i}, beta_{i}, \"{s}\");\n")
            }
            _ => format!(
                r#"for (let j = 0; j < {m}; j++) {{
  acc += step_{i}(j);
}}
"#
            ),
        };
        out.push_str(&block);
    }
}

static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
    let raw = RawGrammar::from_json(&grammar_loader::load_arborium_grammar_json(
        "arborium-javascript",
    ))
    .expect("javascript grammar fixture");
    Grammar::from_raw(&raw).expect("javascript grammar metadata")
});

static SMALL: LazyLock<String> = LazyLock::new(|| corpus::generate(16 * 1024));
static MEDIUM: LazyLock<String> = LazyLock::new(|| corpus::generate(256 * 1024));
static LARGE: LazyLock<String> = LazyLock::new(|| corpus::generate(1024 * 1024));

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse corpus")
}

fn compile(query: &str) -> Module {
    let compiled = QueryBuilder::from_inline(query)
        .compile(&GRAMMAR)
        .expect("bench query parses");
    // On failure, run the query through `plotnik check` to see diagnostics.
    assert!(compiled.is_valid(), "bench query compiles cleanly");
    Module::load(compiled.bytecode().expect("valid query emits bytecode"))
        .expect("emitted bytecode loads")
}

struct Scenario {
    name: &'static str,
    entry: &'static str,
    query: &'static str,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "scan_rows",
        entry: "Items",
        query: indoc! {r#"
            Items = (program [
              Fn: (function_declaration) @fn
              Cls: (class_declaration) @cls
              Lex: (lexical_declaration (variable_declarator name: (identifier) @name))
              Other: (_)
            ]* @items)
        "#},
    },
    Scenario {
        name: "fn_params",
        entry: "Fns",
        query: indoc! {r#"
            Fns = (program [
              Fn: (function_declaration
                name: (identifier) @name
                parameters: (formal_parameters (identifier)* @params))
              Skip: (_)
            ]* @fns)
        "#},
    },
    Scenario {
        name: "deep_calls",
        entry: "Chains",
        query: indoc! {r#"
            Nest = [
              Deeper: (call_expression arguments: (arguments (Nest) @next))
              Leaf: (call_expression)
            ]
            Chains = (program [
              Hit: (lexical_declaration (variable_declarator value: (arrow_function body: (Nest) @chain)))
              Skip: (_)
            ]* @rows)
        "#},
    },
    Scenario {
        name: "pred_eq",
        entry: "Strs",
        query: indoc! {r#"
            Strs = (program [
              Hit: (expression_statement (call_expression arguments: (arguments (string (string_fragment == "needle")) @s)))
              Other: (_)
            ]* @rows)
        "#},
    },
    Scenario {
        name: "pred_regex",
        entry: "Strs",
        query: indoc! {r#"
            Strs = (program [
              Hit: (expression_statement (call_expression arguments: (arguments (string (string_fragment =~ /^ne+dle$/)) @s)))
              Other: (_)
            ]* @rows)
        "#},
    },
];

static ALL_SIZES: [(&str, &LazyLock<String>); 3] =
    [("small", &SMALL), ("medium", &MEDIUM), ("large", &LARGE)];
static MEDIUM_ONLY: [(&str, &LazyLock<String>); 1] = [("medium", &MEDIUM)];

fn corpora_for(name: &str) -> &'static [(&'static str, &'static LazyLock<String>)] {
    match name {
        "scan_rows" => &ALL_SIZES,
        _ => &MEDIUM_ONLY,
    }
}

fn bench_parse(c: &mut Criterion) {
    let source: &str = &MEDIUM;
    let mut group = c.benchmark_group("parse");
    group.throughput(Throughput::Bytes(source.len() as u64));
    group.bench_function("medium", |b| b.iter(|| parse_js(black_box(source))));
    group.finish();
}

fn bench_scenarios(c: &mut Criterion) {
    for s in SCENARIOS {
        let module = compile(s.query);
        let entry = module.entrypoint(s.entry).expect("bench entrypoint exists");
        let mut group = c.benchmark_group(s.name);
        for (size_name, source) in corpora_for(s.name) {
            let source: &str = source;
            let tree = parse_js(source);
            group.throughput(Throughput::Bytes(source.len() as u64));
            group.bench_function(*size_name, |b| {
                b.iter(|| {
                    let vm = VM::builder(source, &tree).build();
                    let effects = vm.execute(&module, &entry).expect("bench query matches");
                    materialize_verified(
                        source,
                        &module,
                        &entry,
                        effects.as_slice(),
                        Colors::new(false),
                    )
                })
            });
        }
        group.finish();
    }
}

/// The scan without materialization: subtracting this from `scan_rows/medium`
/// attributes time between the VM proper and output building.
fn bench_scan_execute(c: &mut Criterion) {
    let s = &SCENARIOS[0];
    let module = compile(s.query);
    let entry = module.entrypoint(s.entry).expect("bench entrypoint exists");
    let source: &str = &MEDIUM;
    let tree = parse_js(source);
    let mut group = c.benchmark_group("scan_rows_execute");
    group.throughput(Throughput::Bytes(source.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            let vm = VM::builder(source, &tree).build();
            let effects = vm.execute(&module, &entry).expect("bench query matches");
            black_box(effects.as_slice().len());
        })
    });
    group.finish();
}

/// Raw dispatch + backtrack throughput. The anchorless star lets every `(_)`
/// element re-bind to any later sibling on backtrack, so the search space is
/// exponential and the run always exhausts its step budget; a fixed budget
/// turns that storm into a stable time-per-step measurement.
fn bench_backtrack_storm(c: &mut Criterion) {
    const STEP_BUDGET: u64 = 50_000;
    let module = compile("Missing = (program {(_)* (debugger_statement)})");
    let entry = module
        .entrypoint("Missing")
        .expect("bench entrypoint exists");
    let source: &str = &SMALL;
    let tree = parse_js(source);
    let mut group = c.benchmark_group("backtrack_storm");
    group.throughput(Throughput::Elements(STEP_BUDGET));
    group.bench_function("50k_steps", |b| {
        b.iter(|| {
            let vm = VM::builder(source, &tree)
                .limits(RuntimeLimitSpec {
                    steps: Limit::Of(STEP_BUDGET),
                    memory: Limit::Auto,
                })
                .build();
            let outcome = vm.execute(&module, &entry);
            assert!(outcome.is_err(), "storm query must exhaust its budget");
        })
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_parse,
    bench_scenarios,
    bench_scan_execute,
    bench_backtrack_storm
);
criterion_main!(benches);
