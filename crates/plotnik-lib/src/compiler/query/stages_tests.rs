use crate::compiler::test_utils::javascript_grammar as javascript;
use indoc::indoc;
use std::fmt::Write as _;

use crate::compiler::diagnostics::DiagnosticKind;
use crate::compiler::{SourceMap, SourcePath};

use super::{LinkOutcome, Query, QueryBuilder};

macro_rules! expect_invalid {
    ($($name:literal: $content:literal),+ $(,)?) => {{
        let mut source_map = SourceMap::new();
        $(source_map.add_file(SourcePath::new($name), $content);)+
        let query = QueryBuilder::new(source_map).analyze().unwrap();
        if query.is_valid() {
            panic!("Expected invalid query, got valid");
        }
        query.dump_diagnostics()
    }};
}

#[test]
fn structural_validation_failure_stops_later_analysis() {
    let query = Query::expect("Q = {. (Missing)}");

    assert!(!query.is_valid());
    let kinds: Vec<_> = query.diagnostics().kinds().collect();
    assert!(kinds.contains(&DiagnosticKind::AnchorWithoutContext));
    assert!(!kinds.contains(&DiagnosticKind::UndefinedReference));
    assert_eq!(query.dump_symbols(), "");
}

impl Query {
    #[track_caller]
    fn parse_and_validate(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).analyze().unwrap();
        if !query.is_valid() {
            panic!(
                "Expected valid query, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        QueryBuilder::new(source_map).analyze().unwrap()
    }

    #[track_caller]
    pub fn expect_valid_ast(src: &str) -> String {
        Self::parse_and_validate(src).dump_ast()
    }

    #[track_caller]
    pub(crate) fn expect_valid_linking(src: &str) -> LinkOutcome {
        let query = Self::parse_and_validate(src).link(javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect_invalid(src: &str) -> String {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).analyze().unwrap();
        if query.is_valid() {
            panic!("Expected invalid query, got valid:\n{}", query.dump_cst());
        }
        query.dump_diagnostics()
    }
}

#[test]
fn parse_errors_do_not_admit_analysis() {
    let query = Query::expect("Q = (identifier");

    assert!(!query.is_valid());
    assert!(query.analysis().is_none());
    assert!(query.dump_cst().contains("Root"));
}

#[test]
fn structural_validation_errors_do_not_admit_analysis() {
    let query = Query::expect("Q = {. (identifier)}");

    assert!(!query.is_valid());
    assert!(query.analysis().is_none());
    assert!(query.dump_cst().contains("Root"));
}

#[test]
fn invalid_three_way_mutual_recursion_across_files() {
    let res = expect_invalid! {
        "a.ptk": "A = (a (B))",
        "b.ptk": "B = (b (C))",
        "c.ptk": "C = (c (A))",
    };

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: no escape path
     --> c.ptk:1:9
      |
    1 | C = (c (A))
      | -       ^
      | |       |
      | |       references A
      | C is defined here
      |
     ::: a.ptk:1:9
      |
    1 | A = (a (B))
      |         - references B
      |
     ::: b.ptk:1:9
      |
    1 | B = (b (C))
      |         - references C (completing cycle)
      |
    help: add a non-recursive branch to terminate: `[Base: ... Rec: (Self)]`
    ");
}

#[test]
fn analysis_rejects_byte_oriented_regex() {
    let linked = Query::expect(r"Q = (identifier =~ /(?-u:\xFF)/) @x").link(javascript());
    let diag = linked.dry_run();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(
        rendered.contains("pattern can match invalid UTF-8"),
        "{rendered}"
    );
}

#[test]
fn dry_run_rejects_value_less_definition() {
    let linked = Query::expect("Q = .").link(javascript());
    let diag = linked.dry_run();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("no entrypoint"), "{rendered}");
}

#[test]
fn dry_run_accepts_valid_query() {
    let linked = Query::parse_and_validate("Q = (identifier) @id").link(javascript());
    assert!(!linked.dry_run().has_errors());
}

#[test]
fn dry_run_flags_dropped_value_less_def_among_valid() {
    let linked = Query::expect(indoc!(
        "
        Bad = .
        Good = (identifier) @id
    "
    ))
    .link(javascript());
    let diag = linked.dry_run();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("no entrypoint"), "{rendered}");
}

#[test]
fn dry_run_is_total_on_empty_source_map() {
    // The dry run must never panic — even on a query with zero sources.
    let linked = QueryBuilder::new(SourceMap::new())
        .link(javascript())
        .unwrap();
    assert!(!linked.dry_run().has_errors());
}

#[test]
fn multifile_link_field_error_in_referenced_body_spans_two_files() {
    // `Foo`'s body is a bare field — valid on its own (no parent to validate it
    // against). Only when `Bar` places `(Foo)` under `call_expression` does the
    // field-on-node-kind check fire, while validation has crossed into a.ptk. The
    // primary span must resolve against a.ptk (where `name:` is written) and the
    // related note against b.ptk (the parent node). Each `Located` node carries its
    // own source, so the split survives crossing the reference between files.
    let mut source_map = SourceMap::new();
    source_map.add_file(SourcePath::new("a.ptk"), "Foo = name: (identifier)");
    source_map.add_file(SourcePath::new("b.ptk"), "Bar = (call_expression (Foo))");
    let analyzed = QueryBuilder::new(source_map).analyze().unwrap();
    assert!(
        analyzed.is_valid(),
        "expected analysis to pass:\n{}",
        analyzed.dump_diagnostics()
    );
    let linked = analyzed.link(javascript());
    assert!(!linked.is_valid(), "expected linking to fail");
    let res = linked.dump_diagnostics();

    insta::assert_snapshot!(res, @"
    error: field `name` is not valid on this node kind
     --> a.ptk:1:7
      |
    1 | Foo = name: (identifier)
      |       ^^^^
      |
     ::: b.ptk:1:8
      |
    1 | Bar = (call_expression (Foo))
      |        --------------- on `call_expression`
      |
    help: valid fields for `call_expression`: `arguments`, `function`, `optional_chain`
    ");
}

#[test]
fn multifile_ref_to_body_with_internal_error_attributes_to_defining_file() {
    // The duplicate-capture error lives inside `Foo`'s body in a.ptk; b.ptk only
    // references `Foo`. Inference no longer descends across the reference, so the
    // error is emitted by `Foo`'s own pass and must resolve against a.ptk — never
    // b.ptk, whose offsets don't even reach that far.
    let res = expect_invalid! {
        "a.ptk": "Foo = (program (identifier) @x (identifier) @x)",
        "b.ptk": "Bar = (Foo)",
    };

    insta::assert_snapshot!(res, @"
    error: capture `@x` already defined in this scope
     --> a.ptk:1:32
      |
    1 | Foo = (program (identifier) @x (identifier) @x)
      |                                ^^^^^^^^^^^^^^^
      |
    help: rename one capture, or use an enum if they are mutually exclusive branches
    ");
}

#[test]
fn deep_reference_chain_hits_recursion_limit() {
    // `A0 = (A1)`, `A1 = (A2)`, … is flat at every definition, so the parser's nesting
    // cap never sees it — but dependency analysis recurses one frame per link, and a
    // chain longer than `max_depth` would overflow the native stack. It is rejected with
    // the same recursion-limit error deep nesting raises, on every platform.
    let depth = 100;
    let mut src = String::new();
    for i in 0..depth {
        writeln!(src, "A{i} = (A{})", i + 1).unwrap();
    }
    writeln!(src, "A{depth} = (identifier)").unwrap();

    let result = QueryBuilder::from_inline(&src)
        .with_parse_max_depth(50)
        .analyze();

    match result {
        Err(crate::compiler::Error::RecursionLimitExceeded) => {}
        Err(other) => panic!("expected RecursionLimitExceeded, got {other:?}"),
        Ok(_) => panic!("expected RecursionLimitExceeded, but analysis succeeded"),
    }
}

#[test]
fn deeply_referenced_alternation_compiles_in_linear_time() {
    // Each level is an alternation naming the previous definition twice, so the inlined
    // form doubles per level — 2^40 nodes. The anchor classifier (run during lowering)
    // walks alternation branches, so without memoization it would never finish. Memoized,
    // each definition is classified once and the whole pipeline stays linear; this test
    // completing at all is the regression guard.
    let depth = 40;
    let mut src = String::new();
    writeln!(src, "Top = (A{depth})").unwrap();
    writeln!(src, "A1 = [(identifier) (identifier)]").unwrap();
    for i in 2..=depth {
        writeln!(src, "A{i} = [(A{p}) (A{p})]", p = i - 1).unwrap();
    }

    let linked = Query::parse_and_validate(&src).link(javascript());
    assert!(linked.is_valid(), "{}", linked.dump_diagnostics());
    assert!(
        !linked.dry_run().has_errors(),
        "alternation DAG must lower cleanly",
    );
}

#[test]
fn satisfiability_step_budget_rejects_and_is_tunable() {
    // A wide child list drives satisfiability construction and the solve's quadratic fixed point.
    // Under a deliberately tiny budget it trips and the query is rejected as too complex; under
    // the default it compiles — so the knob fails closed yet stays out of the way.
    let mut src = String::from("Q = (program");
    for i in 0..60 {
        src.push_str(&format!(" (expression_statement) @c{i}"));
    }
    src.push(')');

    let build = |budget: Option<u64>| {
        let mut source_map = SourceMap::new();
        source_map.add_file(SourcePath::new("q.ptk"), &src);
        let mut builder = QueryBuilder::new(source_map);
        if let Some(budget) = budget {
            builder = builder.with_satisfiability_step_budget(budget);
        }
        builder.analyze().unwrap().link(javascript())
    };

    let tight = build(Some(100));
    assert!(!tight.is_valid(), "a 100-step budget must reject this list");
    assert!(
        tight
            .diagnostics()
            .kinds()
            .any(|k| k == DiagnosticKind::QueryTooComplex),
        "expected QueryTooComplex:\n{}",
        tight.dump_diagnostics(),
    );

    let relaxed = build(None);
    assert!(
        relaxed.is_valid(),
        "the default budget must admit it:\n{}",
        relaxed.dump_diagnostics(),
    );
}

#[test]
fn satisfiability_budget_counts_automaton_construction() {
    let linked = QueryBuilder::new(SourceMap::from_inline(
        "Q = (program (expression_statement))",
    ))
    .with_satisfiability_step_budget(1)
    .analyze()
    .unwrap()
    .link(javascript());

    assert!(!linked.is_valid());
    assert!(
        linked
            .diagnostics()
            .kinds()
            .any(|kind| kind == DiagnosticKind::QueryTooComplex),
        "expected QueryTooComplex:\n{}",
        linked.dump_diagnostics(),
    );
}

#[test]
fn primary_satisfiability_budget_exhaustion_reports_too_complex_once() {
    let mut source_map = SourceMap::new();
    source_map.add_file(
        SourcePath::new("q.ptk"),
        indoc! {"
            Q0 = (array .! (identifier))
            Q1 = (array .! (identifier))
            Q2 = (array .! (identifier))
        "},
    );

    let linked = QueryBuilder::new(source_map)
        .with_satisfiability_step_budget(2)
        .analyze()
        .unwrap()
        .link(javascript());

    let kinds: Vec<_> = linked.diagnostics().kinds().collect();
    assert!(
        kinds.contains(&DiagnosticKind::QueryTooComplex),
        "expected QueryTooComplex:\n{}",
        linked.dump_diagnostics(),
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|&&kind| kind == DiagnosticKind::QueryTooComplex)
            .count(),
        1,
        "primary budget exhaustion must stop the pass:\n{}",
        linked.dump_diagnostics(),
    );
    assert!(
        !kinds.contains(&DiagnosticKind::UnsatisfiablePattern),
        "resource exhaustion must not masquerade as unsatisfiable:\n{}",
        linked.dump_diagnostics(),
    );
}

#[test]
fn exhausted_anchor_probe_budget_keeps_unsatisfiable_verdict() {
    let mut source_map = SourceMap::new();
    source_map.add_file(
        SourcePath::new("q.ptk"),
        indoc! {"
            Q0 = (array .! (identifier))
            Q1 = (array .! (identifier))
            Q2 = (array .! (identifier))
        "},
    );

    let linked = QueryBuilder::new(source_map)
        .with_satisfiability_step_budget(2_000)
        .analyze()
        .unwrap()
        .link(javascript());

    let kinds: Vec<_> = linked.diagnostics().kinds().collect();
    assert_eq!(
        kinds,
        vec![
            DiagnosticKind::UnsatisfiablePattern,
            DiagnosticKind::UnsatisfiablePattern
        ],
        "diagnostic probes must not replace a proven rejection:\n{}",
        linked.dump_diagnostics(),
    );
    let diagnostics = linked.dump_diagnostics();
    assert!(
        diagnostics.contains("matching this child structure"),
        "exhausted anchor probes should fall back to a generic unsatisfiable diagnostic:\n{diagnostics}",
    );
    assert!(
        !kinds.contains(&DiagnosticKind::QueryTooComplex),
        "diagnostic probe exhaustion must not masquerade as query complexity:\n{diagnostics}",
    );
}

#[test]
fn multifile_field_with_ref_to_seq_error() {
    let res = expect_invalid! {
        "defs.ptk": "X = {(a) (b)}",
        "main.ptk": "Q = (call name: (X))",
    };

    insta::assert_snapshot!(res, @"
    error: field `name` cannot match a sequence
     --> main.ptk:1:17
      |
    1 | Q = (call name: (X))
      |                 ^^^
      |
     ::: defs.ptk:1:5
      |
    1 | X = {(a) (b)}
      |     --------- defined here
      |
    help: a field holds a single child node; match one pattern, or move the sequence outside the field
    ");
}
