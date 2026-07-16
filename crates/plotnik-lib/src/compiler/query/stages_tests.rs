use crate::compiler::test_utils::synthetic_grammar as grammar;
use indoc::indoc;
use std::fmt::Write as _;

use crate::compiler::analyze::types::FieldCompletion;
use crate::compiler::diagnostics::DiagnosticKind;
use crate::compiler::{SourceMap, SourcePath};

use super::{BindOutcome, Query, QueryBuilder};

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
    let query = Query::expect("Q = {. (identifier) @x (identifier) @x}");

    assert!(!query.is_valid());
    let kinds: Vec<_> = query.diagnostics().kinds().collect();
    assert!(kinds.contains(&DiagnosticKind::AnchorWithoutContext));
    assert!(!kinds.contains(&DiagnosticKind::DuplicateCaptureInScope));
    assert_eq!(query.dump_symbols(), "");
}

#[test]
fn exported_boundary_anchor_is_a_non_selectable_fragment() {
    let query = Query::parse_and_validate(indoc!(
        r#"
        Tail = {
          (identifier == "a")?
          .
        }
        Q = (array
          "["
          (Tail)
          (identifier == "b")
          "]"
        )
        "#
    ));
    let analysis = query.analysis().expect("valid query has analysis");
    let tail = analysis
        .dependency_analysis
        .def_id_for_name(&analysis.interner, "Tail")
        .expect("Tail is defined");
    let q = analysis
        .dependency_analysis
        .def_id_for_name(&analysis.interner, "Q")
        .expect("Q is defined");

    assert_eq!(
        analysis.type_analysis.expect_def_root_extent(tail),
        crate::compiler::analyze::types::RootExtent::Other
    );
    assert!(analysis.type_analysis.def_requires_anchor_context(tail));
    assert!(!analysis.type_analysis.is_selectable_definition(tail));
    assert!(!analysis.type_analysis.def_requires_anchor_context(q));
    assert!(analysis.type_analysis.is_selectable_definition(q));
}

#[test]
fn single_node_exported_boundary_anchor_is_still_a_fragment() {
    let query = Query::parse_and_validate(indoc!(
        "
        Tail = {(identifier) .}
        Q = (array \"[\" (Tail) \"]\")
        "
    ));
    let analysis = query.analysis().expect("valid query has analysis");
    let tail = analysis
        .dependency_analysis
        .def_id_for_name(&analysis.interner, "Tail")
        .expect("Tail is defined");

    assert_eq!(
        analysis.type_analysis.expect_def_root_extent(tail),
        crate::compiler::analyze::types::RootExtent::SingleNode
    );
    assert!(analysis.type_analysis.def_requires_anchor_context(tail));
    assert!(!analysis.type_analysis.is_selectable_definition(tail));
}

#[test]
fn compiled_query_exports_only_the_context_free_caller() {
    let compiled = Query::parse_and_validate(indoc!(
        "
        Tail = {(identifier) .}
        Q = (array \"[\" (Tail) \"]\")
        "
    ))
    .bind(grammar())
    .compile()
    .expect("compilation answers");

    assert!(
        compiled.is_valid(),
        "expected valid compilation:\n{}",
        compiled.diagnostics().render(compiled.source_map())
    );
    assert_eq!(compiled.entry_point_names().collect::<Vec<_>>(), ["Q"]);
}

#[test]
fn nullable_neighbor_exposes_an_otherwise_interior_anchor() {
    let query = Query::expect("Q = {(a)? . (b)}");

    assert!(!query.is_valid());
    assert!(
        query
            .diagnostics()
            .kinds()
            .any(|kind| kind == DiagnosticKind::AnchorWithoutContext)
    );
}

#[test]
fn nested_group_inherits_its_consuming_neighbor_context() {
    Query::parse_and_validate("Q = {(a) {. (b)}}");
}

#[test]
fn contextless_alias_chain_reports_the_authored_anchor() {
    let query = Query::expect(indoc!(
        "
        Tail = {(identifier) .}
        Alias = (Tail)
        Q = (Alias)
        "
    ));

    assert!(!query.is_valid());
    let rendered = query.dump_diagnostics();
    assert!(
        rendered.contains("anchor needs an enclosing node"),
        "{rendered}"
    );
    assert!(rendered.contains("Tail = {(identifier) .}"), "{rendered}");
}

#[test]
fn recursive_context_contract_keeps_the_authored_anchor_origin() {
    let query = Query::expect(indoc!(
        "
        Tail = [
          (identifier)
          {(identifier) (Tail)? .}
        ]
        Alias = (Tail)
        "
    ));

    assert!(!query.is_valid());
    let rendered = query.dump_diagnostics();
    assert!(
        rendered.contains("anchor needs an enclosing node"),
        "{rendered}"
    );
    assert!(rendered.contains("{(identifier) (Tail)? .}"), "{rendered}");
}

#[test]
fn named_node_discharges_a_recursive_exported_anchor() {
    Query::parse_and_validate(indoc!(
        "
        Tail = [
          (identifier)
          {(identifier) (Tail)? .}
        ]
        Q = (array \"[\" (Tail) \"]\")
        "
    ));
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
    pub(crate) fn expect_valid_binding(src: &str) -> BindOutcome {
        let query = Self::parse_and_validate(src).bind(grammar());
        if !query.is_valid() {
            panic!(
                "Expected valid grammar binding, got error:\n{}",
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

fn field_completion(src: &str, field: &str) -> FieldCompletion {
    let query = Query::parse_and_validate(src);
    let analysis = query.analysis().expect("valid query must have analysis");
    let pattern = analysis
        .symbol_table
        .body("Q")
        .expect("test query must define Q");
    let field = analysis
        .interner
        .get(field)
        .expect("test field must be interned");

    analysis
        .type_analysis
        .expect_field_completions(pattern)
        .completion(field)
}

#[test]
fn analysis_records_every_field_completion() {
    assert_eq!(
        field_completion("Q = [(a) @value (b) @value]", "value"),
        FieldCompletion::AlwaysPresent,
    );
    assert_eq!(
        field_completion("Q = [(a) @value (b)]", "value"),
        FieldCompletion::Absent,
    );
    assert_eq!(
        field_completion("Q = [(a)* @items (b)]", "items"),
        FieldCompletion::EmptyList,
    );
    assert_eq!(
        field_completion("Q = [(a) @present :: bool (b)]", "present"),
        FieldCompletion::False,
    );
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

    insta::assert_snapshot!(res, @"
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
    help: add a non-recursive alternative to terminate: `[Base: ... Rec: (Self)]`
    ");
}

#[test]
fn analysis_rejects_byte_oriented_regex() {
    let bound = Query::expect(r"Q = (identifier =~ /(?-u:\xFF)/) @x").bind(grammar());
    let compiled = bound.compile().expect("compilation answers");
    let diag = compiled.diagnostics();
    assert!(diag.has_errors());
    let rendered = diag.render(compiled.source_map());
    assert!(
        rendered.contains("pattern can match invalid UTF-8"),
        "{rendered}"
    );
}

#[test]
fn compile_rejects_definition_with_positional_only_body() {
    let bound = Query::expect("Q = .").bind(grammar());
    let compiled = bound.compile().expect("compilation answers");
    let diag = compiled.diagnostics();
    assert!(diag.has_errors());
    let rendered = diag.render(compiled.source_map());
    assert!(rendered.contains("no selectable entry point"), "{rendered}");
}

#[test]
fn compile_accepts_valid_query() {
    let bound = Query::parse_and_validate("Q = (identifier) @id").bind(grammar());
    assert!(
        !bound
            .compile()
            .expect("compilation answers")
            .diagnostics()
            .has_errors()
    );
}

#[test]
fn compile_flags_positional_only_definition_among_valid_definitions() {
    let bound = Query::expect(indoc!(
        "
        Bad = .
        Good = (identifier) @id
    "
    ))
    .bind(grammar());
    let compiled = bound.compile().expect("compilation answers");
    let diag = compiled.diagnostics();
    assert!(diag.has_errors());
    let rendered = diag.render(compiled.source_map());
    assert!(rendered.contains("no selectable entry point"), "{rendered}");
}

#[test]
fn compile_is_total_on_empty_source_map() {
    let bound = QueryBuilder::new(SourceMap::new()).bind(grammar()).unwrap();
    assert!(
        !bound
            .compile()
            .expect("compilation answers")
            .diagnostics()
            .has_errors()
    );
}

#[test]
fn multifile_bind_field_error_in_referenced_body_spans_two_files() {
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
    let bound = analyzed.bind(grammar());
    assert!(!bound.is_valid(), "expected grammar binding to fail");
    let res = bound.dump_diagnostics();

    insta::assert_snapshot!(res, @"
    error: grammar field `name` is not valid on this node kind
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
    help: valid grammar fields for `call_expression`: `arguments`, `function`, `optional_chain`
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
    help: rename one capture, or use labeled alternatives to preserve which alternative matched
    ");
}

#[test]
fn deep_reference_chain_hits_recursion_limit() {
    // `A0 = (A1)`, `A1 = (A2)`, … is flat at every definition, so dependency analysis
    // owns its own stack ceiling, and an over-deep chain gets the same fatal
    // recursion-limit error as over-deep source nesting.
    let depth = 100;
    let mut src = String::new();
    for i in 0..depth {
        writeln!(src, "A{i} = (A{})", i + 1).unwrap();
    }
    writeln!(src, "A{depth} = (identifier)").unwrap();

    let result = QueryBuilder::from_inline(&src)
        .with_reference_max_depth(50)
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
    // walks alternation alternatives, so without memoization it would never finish. Memoized,
    // each definition is classified once and the whole pipeline stays linear; this test
    // completing at all is the regression guard.
    let depth = 40;
    let mut src = String::new();
    writeln!(src, "Top = (A{depth})").unwrap();
    writeln!(src, "A1 = [(identifier) (identifier)]").unwrap();
    for i in 2..=depth {
        writeln!(src, "A{i} = [(A{p}) (A{p})]", p = i - 1).unwrap();
    }

    let bound = Query::parse_and_validate(&src).bind(grammar());
    assert!(bound.is_valid(), "{}", bound.dump_diagnostics());
    assert!(
        !bound
            .compile()
            .expect("compilation answers")
            .diagnostics()
            .has_errors(),
        "alternation DAG must lower cleanly",
    );
}

#[test]
fn satisfiability_work_budget_rejects_and_is_tunable() {
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
            builder = builder.with_satisfiability_work_budget(budget);
        }
        builder.analyze().unwrap().bind(grammar())
    };

    let tight = build(Some(100));
    assert!(
        !tight.is_valid(),
        "a 100-unit work budget must reject this list"
    );
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
    let bound = QueryBuilder::new(SourceMap::from_inline(
        "Q = (program (expression_statement))",
    ))
    .with_satisfiability_work_budget(1)
    .analyze()
    .unwrap()
    .bind(grammar());

    assert!(!bound.is_valid());
    assert!(
        bound
            .diagnostics()
            .kinds()
            .any(|kind| kind == DiagnosticKind::QueryTooComplex),
        "expected QueryTooComplex:\n{}",
        bound.dump_diagnostics(),
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

    let bound = QueryBuilder::new(source_map)
        .with_satisfiability_work_budget(2)
        .analyze()
        .unwrap()
        .bind(grammar());

    let kinds: Vec<_> = bound.diagnostics().kinds().collect();
    assert!(
        kinds.contains(&DiagnosticKind::QueryTooComplex),
        "expected QueryTooComplex:\n{}",
        bound.dump_diagnostics(),
    );
    assert_eq!(
        kinds
            .iter()
            .filter(|&&kind| kind == DiagnosticKind::QueryTooComplex)
            .count(),
        1,
        "primary budget exhaustion must stop the pass:\n{}",
        bound.dump_diagnostics(),
    );
    assert!(
        !kinds.contains(&DiagnosticKind::UnsatisfiablePattern),
        "resource exhaustion must not masquerade as unsatisfiable:\n{}",
        bound.dump_diagnostics(),
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

    let bound = QueryBuilder::new(source_map)
        .with_satisfiability_work_budget(20)
        .analyze()
        .unwrap()
        .bind(grammar());

    let kinds: Vec<_> = bound.diagnostics().kinds().collect();
    assert!(
        !kinds.is_empty()
            && kinds
                .iter()
                .all(|&kind| kind == DiagnosticKind::UnsatisfiablePattern),
        "diagnostic probes must not replace a proven rejection:\n{}",
        bound.dump_diagnostics()
    );
    let diagnostics = bound.dump_diagnostics();
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
    error: grammar field `name` cannot match a sequence
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
    help: a grammar field holds a single child node; match one pattern, or move the sequence outside the grammar-field constraint
    ");
}
