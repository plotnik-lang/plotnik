use plotnik_bytecode::{Module, dump};
use plotnik_core::Colors;
use plotnik_core::grammar::{Grammar, raw::RawGrammar};
use std::sync::LazyLock;

use crate::SourceMap;

use super::{GrammarBoundQuery, Query, QueryBuilder};

fn javascript() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(include_str!(env!(
            "PLOTNIK_COMPILER_JAVASCRIPT_GRAMMAR_JSON"
        )))
        .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });

    &GRAMMAR
}

macro_rules! expect_invalid {
    ($($name:literal: $content:literal),+ $(,)?) => {{
        let mut source_map = SourceMap::new();
        $(source_map.add_file($name, $content);)+
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if query.is_valid() {
            panic!("Expected invalid query, got valid");
        }
        query.dump_diagnostics()
    }};
}

impl Query {
    #[track_caller]
    fn parse_and_validate(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if !query.is_valid() {
            panic!(
                "Expected valid query, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    /// Parse and validate syntax only (no semantic analysis).
    /// Use this for pure parser/grammar tests.
    #[track_caller]
    fn parse_syntax_only(src: &str) -> Self {
        use crate::diagnostics::DiagnosticKind::*;
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        let diag = query.diagnostics();
        let has_parse_error = diag.raw().iter().any(|d| {
            matches!(
                d.kind,
                UnclosedTree
                    | UnclosedSequence
                    | UnclosedAlternation
                    | UnclosedRegex
                    | UnclosedString
                    | ExpectedExpression
                    | ExpectedTypeName
                    | ExpectedFieldName
                    | ExpectedSubtype
                    | ExpectedPredicateValue
                    | EmptyTree
                    | EmptyAnonymousNode
                    | EmptySequence
                    | EmptyAlternation
                    | BareIdentifier
                    | InvalidSeparator
                    | QuantifiedAnchor
                    | CapturedAnchor
                    | UnexpectedToken
            )
        });

        if has_parse_error {
            panic!(
                "Expected valid syntax, got parse error:\n{}",
                query.dump_diagnostics()
            );
        }
        query
    }

    #[track_caller]
    pub fn expect(src: &str) -> Self {
        let source_map = SourceMap::from_inline(src);
        QueryBuilder::new(source_map).parse().unwrap().analyze()
    }

    #[track_caller]
    pub fn expect_valid(src: &str) -> Self {
        Self::parse_and_validate(src)
    }

    #[track_caller]
    pub fn expect_valid_cst(src: &str) -> String {
        Self::parse_and_validate(src).dump_cst()
    }

    /// Parse-only CST dump (for pure parser tests, no semantic validation).
    #[track_caller]
    pub fn parse_cst(src: &str) -> String {
        Self::parse_syntax_only(src).dump_cst()
    }

    #[track_caller]
    pub fn expect_valid_cst_full(src: &str) -> String {
        Self::parse_and_validate(src).dump_cst_full()
    }

    #[track_caller]
    pub fn expect_valid_ast(src: &str) -> String {
        Self::parse_and_validate(src).dump_ast()
    }

    #[track_caller]
    pub fn expect_valid_arities(src: &str) -> String {
        Self::parse_and_validate(src).dump_with_arities()
    }

    #[track_caller]
    pub fn expect_valid_symbols(src: &str) -> String {
        Self::parse_and_validate(src).dump_symbols()
    }

    #[track_caller]
    pub fn expect_valid_linking(src: &str) -> GrammarBoundQuery {
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
    pub fn expect_invalid_linking(src: &str) -> String {
        let query = Self::parse_and_validate(src).link(javascript());
        if query.is_valid() {
            panic!("Expected failed linking, got valid");
        }
        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_valid_types(src: &str) -> String {
        let query = Self::parse_and_validate(src).link(javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid types, got error:\n{}",
                query.dump_diagnostics()
            );
        }

        let bytecode = query.emit().expect("bytecode emission should succeed");
        let module = Module::load(&bytecode).expect("module loading should succeed");
        crate::typegen::typescript::emit(&module, crate::typegen::typescript::Config::default())
    }

    #[track_caller]
    pub fn expect_valid_bytecode(src: &str) -> String {
        let query = Self::parse_and_validate(src).link(javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        let bytecode = query.emit().expect("bytecode emission should succeed");
        let module = Module::load(&bytecode).expect("module loading should succeed");
        dump(&module, Colors::OFF)
    }

    #[track_caller]
    pub fn expect_valid_bytes(src: &str) -> Vec<u8> {
        let query = Self::parse_and_validate(src).link(javascript());
        if !query.is_valid() {
            panic!(
                "Expected valid linking, got error:\n{}",
                query.dump_diagnostics()
            );
        }
        query.emit().expect("bytecode emission should succeed")
    }

    #[track_caller]
    pub fn expect_invalid(src: &str) -> String {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();
        if query.is_valid() {
            panic!("Expected invalid query, got valid:\n{}", query.dump_cst());
        }
        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_warning(src: &str) -> String {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

        if !query.is_valid() {
            panic!(
                "Expected valid query with warning, got error:\n{}",
                query.dump_diagnostics()
            );
        }

        if !query.diagnostics().has_warnings() {
            panic!("Expected warning, got none:\n{}", query.dump_cst());
        }

        query.dump_diagnostics()
    }

    #[track_caller]
    pub fn expect_cst_with_warnings(src: &str) -> String {
        let source_map = SourceMap::from_inline(src);
        let query = QueryBuilder::new(source_map).parse().unwrap().analyze();

        if !query.is_valid() {
            panic!(
                "Expected valid query (warnings ok), got error:\n{}",
                query.dump_diagnostics()
            );
        }

        query.dump_cst()
    }
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
fn check_compile_rejects_enum_zero_width_branch_in_quantifier() {
    // Passes analysis; the emitted bytecode is rejected by `Module::load`
    // (EffectStackImbalance). `check_compile` must report it, not panic.
    let linked = Query::parse_and_validate("Q = (program [A: (comment)? @c]* @items)").link(javascript());
    let diag = linked.check_compile();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("effect stack imbalance"), "{rendered}");
}

#[test]
fn check_compile_rejects_byte_oriented_regex() {
    // Passes analysis; the DFA build fails at emit time (EmitError::RegexCompile).
    let linked = Query::parse_and_validate(r"Q = (identifier =~ /(?-u:\xFF)/) @x").link(javascript());
    let diag = linked.check_compile();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("regex compile error"), "{rendered}");
}

#[test]
fn check_compile_rejects_value_less_definition() {
    // `Q = .` compiles to a module with no entrypoints.
    let linked = Query::parse_and_validate("Q = .").link(javascript());
    let diag = linked.check_compile();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("no entrypoint"), "{rendered}");
}

#[test]
fn check_compile_accepts_valid_query() {
    let linked = Query::parse_and_validate("Q = (identifier) @id").link(javascript());
    assert!(!linked.check_compile().has_errors());
}

#[test]
fn check_compile_flags_dropped_value_less_def_among_valid() {
    // A value-less def silently omitted from the entrypoint table while a sibling
    // def compiles fine must still be reported, not hidden by the sibling.
    let linked = Query::parse_and_validate("Bad = .\nGood = (identifier) @id").link(javascript());
    let diag = linked.check_compile();
    assert!(diag.has_errors());
    let rendered = diag.render(linked.source_map());
    assert!(rendered.contains("no entrypoint"), "{rendered}");
}

#[test]
fn check_compile_is_total_on_empty_source_map() {
    // The dry run must never panic — even on a query with zero sources.
    let linked = QueryBuilder::new(SourceMap::new())
        .parse()
        .unwrap()
        .analyze()
        .link(javascript());
    assert!(!linked.check_compile().has_errors());
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
    source_map.add_file("a.ptk", "Foo = name: (identifier)");
    source_map.add_file("b.ptk", "Bar = (call_expression (Foo))");
    let analyzed = QueryBuilder::new(source_map).parse().unwrap().analyze();
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
    help: rename one capture, or use a labeled alternation if they are mutually exclusive branches
    ");
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
