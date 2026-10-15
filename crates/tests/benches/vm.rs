use std::hint::black_box;
use std::sync::LazyLock;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use indoc::indoc;
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

use plotnik_compiler::bytecode::Module;
use plotnik_compiler::grammar::{Grammar, raw::RawGrammar};
use plotnik_compiler::{Colors, Limit, QueryBuilder, RuntimeLimitSpec, VM, materialize_verified};

#[path = "../test_support/grammar_loader.rs"]
mod grammar_loader;

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
    let raw = RawGrammar::from_json(grammar_loader::javascript_grammar_json())
        .expect("javascript grammar fixture");
    Grammar::from_raw(&raw).expect("javascript grammar metadata")
});

static SMALL: LazyLock<String> = LazyLock::new(|| corpus::generate(16 * 1024));
static MEDIUM: LazyLock<String> = LazyLock::new(|| corpus::generate(256 * 1024));
static LARGE: LazyLock<String> = LazyLock::new(|| corpus::generate(1024 * 1024));

fn parse_js(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = tree_sitter_javascript::LANGUAGE.into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse corpus")
}

fn compile(query: &str) -> Module {
    let compiled = QueryBuilder::from_inline(query)
        .compile(&GRAMMAR)
        .expect("bench query parses");
    assert!(compiled.is_valid(), "bench query compiles cleanly");
    compiled
        .emit(plotnik_compiler::BytecodeConfig::new())
        .expect("bytecode emission answers")
        .into_artifact()
        .expect("valid query emits bytecode")
}

struct Scenario {
    name: &'static str,
    entry: &'static str,
    query: &'static str,
}

const SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "scan_items",
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
            ]* @items)
        "#},
    },
    Scenario {
        name: "pred_eq",
        entry: "Strs",
        query: indoc! {r#"
            Strs = (program [
              Hit: (expression_statement (call_expression arguments: (arguments (string (string_fragment == "needle")) @s)))
              Other: (_)
            ]* @items)
        "#},
    },
    Scenario {
        name: "pred_regex",
        entry: "Strs",
        query: indoc! {r#"
            Strs = (program [
              Hit: (expression_statement (call_expression arguments: (arguments (string (string_fragment =~ /^ne+dle$/)) @s)))
              Other: (_)
            ]* @items)
        "#},
    },
];

static ALL_SIZES: [(&str, &LazyLock<String>); 3] =
    [("small", &SMALL), ("medium", &MEDIUM), ("large", &LARGE)];
static MEDIUM_ONLY: [(&str, &LazyLock<String>); 1] = [("medium", &MEDIUM)];

fn corpora_for(name: &str) -> &'static [(&'static str, &'static LazyLock<String>)] {
    match name {
        "scan_items" => &ALL_SIZES,
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
                    let journal = vm.execute(&module, &entry).expect("bench query matches");
                    materialize_verified(
                        source,
                        &module,
                        &entry,
                        journal.output_events(),
                        Colors::new(false),
                    )
                })
            });
        }
        group.finish();
    }
}

fn bench_scan_execute(c: &mut Criterion) {
    let s = &SCENARIOS[0];
    let module = compile(s.query);
    let entry = module.entrypoint(s.entry).expect("bench entrypoint exists");
    let source: &str = &MEDIUM;
    let tree = parse_js(source);
    let mut group = c.benchmark_group("scan_items_execute");
    group.throughput(Throughput::Bytes(source.len() as u64));
    group.bench_function("medium", |b| {
        b.iter(|| {
            let vm = VM::builder(source, &tree).build();
            let journal = vm.execute(&module, &entry).expect("bench query matches");
            black_box(journal.as_slice().len());
        })
    });
    group.finish();
}

fn bench_backtrack_storm(c: &mut Criterion) {
    const FUEL_BUDGET: u64 = 50_000;
    let module = compile("Missing = (program {(_)* (debugger_statement)})");
    let entry = module
        .entrypoint("Missing")
        .expect("bench entrypoint exists");
    let source: &str = &SMALL;
    let tree = parse_js(source);
    let mut group = c.benchmark_group("backtrack_storm");
    group.throughput(Throughput::Elements(FUEL_BUDGET));
    group.bench_function("50k_dispatches", |b| {
        b.iter(|| {
            let vm = VM::builder(source, &tree)
                .limits(RuntimeLimitSpec {
                    fuel_limit: Limit::Of(FUEL_BUDGET),
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
