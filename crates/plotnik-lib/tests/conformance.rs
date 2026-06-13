//! Conformance corpus for the compiler→VM seam.
//!
//! Each case runs one query against real JavaScript source through the whole
//! public pipeline — `QueryBuilder` → `link` → `emit` → `Module::load` → `VM`
//! → `ValueMaterializer` — and snapshots both the inferred TypeScript types and
//! the execution result in a single file. Every successful execution is checked
//! against its declared type with `debug_verify_type` (active in the debug test
//! build), so a type-unsound emission fails the test instead of slipping through.
//!
//! Adding a case is one `#[test]` plus a `shot_exec!(query, source)` line.
//!
//! Known-broken cases are marked `#[ignore]` (they panic) or carry a `BUG #NNN`
//! note (they return a type-valid but semantically wrong result, so the snapshot
//! characterizes today's behavior and will flip when the bug is fixed).

use std::sync::LazyLock;

use arborium_tree_sitter::{Parser, Tree};
use plotnik_lib::bytecode::Module;
use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use plotnik_lib::typegen::typescript;
use plotnik_lib::{
    Colors, Materializer, QueryBuilder, RuntimeError, SourceMap, VM, ValueMaterializer,
    debug_verify_type,
};

fn javascript() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(include_str!(env!("PLOTNIK_LIB_JAVASCRIPT_GRAMMAR_JSON")))
            .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });
    &GRAMMAR
}

fn parse_javascript(source: &str) -> Tree {
    let language: arborium_tree_sitter::Language = arborium_javascript::language().into();
    let mut parser = Parser::new();
    parser
        .set_language(&language)
        .expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
}

/// Run a query against source and return a combined snapshot of the inferred
/// types and the execution result. Panics (fails the test) on a non-linking
/// query or a type-unsound materialization.
fn run_pipeline(query_src: &str, source_src: &str, entry: &str) -> String {
    let mut source_map = SourceMap::new();
    source_map.add_file("query.ptk", query_src);

    let query = QueryBuilder::new(source_map)
        .parse()
        .expect("query parsing should not exhaust fuel")
        .analyze()
        .link(javascript());
    assert!(query.is_valid(), "query should link: {query_src}");

    let bytes = query.emit().expect("bytecode emission should succeed");
    let module = Module::load(&bytes).expect("module loading should succeed");

    let types =
        typescript::emit_with_config(&module, typescript::Config::new().emit_node_type(false));

    let entrypoint = module
        .entrypoints()
        .find_by_name(entry, &module.strings())
        .unwrap_or_else(|| panic!("entrypoint `{entry}` should exist"));

    let tree = parse_javascript(source_src);
    let vm = VM::builder(source_src, &tree).build();

    let result = match vm.execute(&module, 0, &entrypoint) {
        Ok(effects) => {
            let materializer = ValueMaterializer::new(source_src, module.types(), module.strings());
            let value = materializer.materialize(effects.as_slice(), entrypoint.result_type());

            // Verify the emitted value against its declared type. In the (debug)
            // test build a mismatch panics and fails the case; in release this is
            // a no-op, so production stays zero-cost.
            debug_verify_type(
                &value,
                entrypoint.result_type(),
                &module,
                Colors::new(false),
            );

            value.format(false, Colors::new(false))
        }
        Err(RuntimeError::NoMatch) => "<no match>".to_string(),
        Err(err) => panic!("unexpected runtime error: {err}"),
    };

    format!(
        "==== query ====\n{}\n\n==== source ====\n{}\n\n==== types ====\n{}\n\n==== result ====\n{}\n",
        query_src.trim(),
        source_src,
        types.trim_end(),
        result,
    )
}

macro_rules! shot_exec {
    ($query:expr, $source:expr $(,)?) => {{
        let output = run_pipeline($query, $source, "Q");
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(output);
        });
    }};
    ($query:expr, $source:expr, entry = $entry:expr $(,)?) => {{
        let output = run_pipeline($query, $source, $entry);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(output);
        });
    }};
}

// Alternation search in child / first-child / sibling positions (#407).

#[test]
fn alternation_in_child_position() {
    shot_exec!(r#"Q = (program [(expression_statement) @expr])"#, "a;");
}

#[test]
fn anchor_before_alternation_in_sibling() {
    shot_exec!(
        r#"Q = (program (expression_statement (binary_expression (identifier) @left . [(identifier) @right])))"#,
        "a + b"
    );
}

#[test]
fn anchor_before_alternation_in_first_child() {
    shot_exec!(
        r#"Q = (program (expression_statement (call_expression arguments: (arguments . [(identifier) @arg]))))"#,
        "f(x)"
    );
}

// Soft vs strict anchors and runtime trivia skipping (#411).

#[test]
fn soft_anchor_skips_comment_between_named_nodes() {
    shot_exec!(
        r#"Q = (program (expression_statement (binary_expression (identifier) @left . (identifier) @right)))"#,
        "a + /* c */ b"
    );
}

#[test]
fn soft_anchor_rejects_named_node_between() {
    shot_exec!(
        r#"Q = (program (expression_statement (binary_expression (identifier) @left . (identifier) @right)))"#,
        "a + f() + b"
    );
}

#[test]
fn soft_anchor_skips_extra_but_not_token() {
    shot_exec!(
        r#"Q = (program (expression_statement (array "," . (number) @n)))"#,
        "[1, /* c */ 2]"
    );
}

#[test]
fn strict_anchor_requires_true_adjacency() {
    shot_exec!(
        r#"Q = (program (expression_statement (array "," .! (number) @n)))"#,
        "[1, 2]"
    );
}

#[test]
fn strict_anchor_rejects_comment_between() {
    shot_exec!(
        r#"Q = (program (expression_statement (array "," .! (number) @n)))"#,
        "[1, /* c */ 2]"
    );
}

#[test]
fn soft_anchor_retries_over_intervening_token() {
    // The first `,` can't reach `2` (a non-extra `,` intervenes); the anchored
    // search retries and the second `,` is adjacent to `2`.
    shot_exec!(
        r#"Q = (program (expression_statement (array "," . (number) @n)))"#,
        "[1,,2]"
    );
}

#[test]
fn soft_anchor_no_comma_adjacent_to_number() {
    shot_exec!(
        r#"Q = (program (expression_statement (array "," . (number) @n)))"#,
        "[1,,]"
    );
}

#[test]
fn up_anchor_skips_trailing_comment() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier == "a")) .)"#,
        "a; /* c */"
    );
}

#[test]
fn up_anchor_rejects_trailing_named_node() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier == "a")) .)"#,
        "a; b;"
    );
}

#[test]
fn up_anchor_accepts_anonymous_operand_as_last_child() {
    shot_exec!(
        r#"Q = (program (debugger_statement "debugger" .))"#,
        "debugger /* c */"
    );
}

#[test]
fn up_anchor_rejects_trailing_anonymous_token() {
    shot_exec!(
        r#"Q = (program (debugger_statement "debugger" .))"#,
        "debugger;"
    );
}

#[test]
fn up_strict_anchor_accepts_literal_last_child() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier == "a") .!))"#,
        "a"
    );
}

#[test]
fn up_strict_anchor_rejects_trailing_comment() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier == "a") .!))"#,
        "a /* c */;"
    );
}

#[test]
fn explicit_comment_pattern_matches_before_skip_policy() {
    shot_exec!(
        r#"Q = (program {(comment) @doc . (function_declaration) @fn})"#,
        "// doc\nfunction f() {}"
    );
}

// Interior anchor retry and quantifier match priority (#414).

#[test]
fn interior_anchor_retries_at_later_siblings() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration) @a . (expression_statement) @b})"#,
        "let x; let y; foo;"
    );
}

#[test]
fn greedy_quantifier_exit_binds_following_leftmost() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)* @decls (_) @x})"#,
        "let a; foo; bar;"
    );
}

#[test]
fn non_greedy_plus_matches_leftmost_minimal() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)+? @d})"#,
        "let a; let b;"
    );
}

#[test]
fn quantifier_skips_non_matching_siblings() {
    shot_exec!(
        r#"Q = (program (lexical_declaration)* @decls)"#,
        "let a; foo; let b;"
    );
}

#[test]
fn interior_strict_anchor_rejects_non_adjacent_pair() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration) @a .! (expression_statement) @b})"#,
        "let x; /* c */ foo;"
    );
}

// Capture value semantics: inference vs emission divergences (#420).

/// BUG #420: duplicate capture names inside a named node are accepted and emit
/// invalid JSON with two `x` keys; the sequence path correctly errors. The
/// snapshot characterizes the duplicate-key output until validation lands.
#[test]
fn duplicate_capture_names_in_node() {
    shot_exec!(
        r#"Q = (program (expression_statement (binary_expression (identifier) @x (identifier) @x)))"#,
        "a + b"
    );
}

#[test]
#[ignore = "#420: duplicate tagged-alternation labels collide; emitted value is not the inferred tagged union (verify_value rejects it)"]
fn duplicate_tagged_alternation_labels() {
    shot_exec!(
        r#"Q = (program (expression_statement [A: (identifier) @x  A: (number) @y]))"#,
        "foo;"
    );
}

// IR pass soundness (#421).

#[test]
#[ignore = "#421: collapse_prefix deletes a still-referenced instruction; emit panics with 'label not in layout'"]
fn collapse_prefix_drops_referenced_instruction() {
    shot_exec!(r#"Q = (program (comment)? (comment)? (comment)?)"#, "// c");
}

/// BUG #383: a captured ref that returns via a non-greedy optional skip
/// fabricates the root `program` node for `x` instead of yielding nothing.
#[test]
fn captured_ref_via_non_greedy_optional_skip() {
    shot_exec!("A = (identifier)??\nQ = (A) @x", "foo");
}

/// BUG #441: an anchor before a quantified follower is not enforced, so `b`
/// matches `debugger;` even though `a` was skipped and `b` must be the first
/// child. Expected: no match.
#[test]
fn anchored_quantified_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

/// BUG #439: with a trailing anchor present, the interior anchor after a
/// skippable first item is ignored, so `b` binds past the first child.
/// Expected: no match.
#[test]
fn interior_anchor_ignored_with_trailing_anchor() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement) @b .})"#,
        "foo; debugger;"
    );
}

/// BUG #417: a supertype pattern (`statement` groups concrete kinds and is never
/// itself produced by tree-sitter) silently never matches instead of being
/// rejected at link time, yielding an empty array.
#[test]
fn supertype_pattern_silently_never_matches() {
    shot_exec!(r#"Q = (program (statement)* @s)"#, "let x;");
}
