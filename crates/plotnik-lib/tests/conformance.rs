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

/// Render the diagnostics for a query that must be *rejected* at check time.
///
/// Some `#420` divergences are fixed by adding the missing validation: the query
/// can no longer reach the VM, so it is pinned by its diagnostics instead of a
/// materialized value.
fn run_check(query_src: &str) -> String {
    let mut source_map = SourceMap::new();
    source_map.add_file("query.ptk", query_src);

    let query = QueryBuilder::new(source_map)
        .parse()
        .expect("query parsing should not exhaust fuel")
        .analyze()
        .link(javascript());
    assert!(
        !query.is_valid(),
        "query should be rejected at check time: {query_src}"
    );

    format!(
        "==== query ====\n{}\n\n==== diagnostics ====\n{}",
        query_src.trim(),
        query.diagnostics().render(query.source_map()).trim_end(),
    )
}

macro_rules! shot_check {
    ($query:expr $(,)?) => {{
        let output = run_check($query);
        insta::with_settings!({ omit_expression => true }, {
            insta::assert_snapshot!(output);
        });
    }};
}

/// Render diagnostics for a multi-file workspace that must be *rejected*.
///
/// Pins the cross-file source attribution fix (#420 #8): a reference into
/// another workspace file used to be walked under the *referrer's* source id,
/// so a diagnostic raised inside the referenced file carried the wrong source
/// and sliced foreign content out of bounds (panic at link, mis-attribution at
/// render). Each file is `(display_name, content)`.
fn run_workspace_check(files: &[(&str, &str)]) -> String {
    let mut source_map = SourceMap::new();
    for (name, content) in files {
        source_map.add_file(name, content);
    }

    let query = QueryBuilder::new(source_map)
        .parse()
        .expect("query parsing should not exhaust fuel")
        .analyze()
        .link(javascript());
    assert!(
        !query.is_valid(),
        "workspace should be rejected at check time"
    );

    let mut out = String::from("==== files ====\n");
    for (name, content) in files {
        out.push_str(&format!("# {name}\n{}\n", content.trim()));
    }
    out.push_str("\n==== diagnostics ====\n");
    out.push_str(query.diagnostics().render(query.source_map()).trim_end());
    out
}

macro_rules! shot_workspace_check {
    ($files:expr $(,)?) => {{
        let output = run_workspace_check($files);
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

/// #420: duplicate capture names inside a named node are now rejected at check
/// time. Previously the named-node path used `or_insert` (silent) while the
/// sequence path errored, so this emitted invalid JSON with two `x` keys.
#[test]
fn duplicate_capture_names_in_node() {
    shot_check!(
        r#"Q = (program (expression_statement (binary_expression (identifier) @x (identifier) @x)))"#
    );
}

/// #420: duplicate tagged-alternation labels are now rejected at check time.
/// Previously they collided in a `BTreeMap`, leaving the enum with one variant
/// while the emitter produced a value the type verifier rejected.
#[test]
fn duplicate_tagged_alternation_labels() {
    shot_check!(r#"Q = (program (expression_statement [A: (identifier) @x  A: (number) @y]))"#);
}

/// #420 #3: a captured tagged alternation now materializes the tagged union the
/// types promise. Inference and emission both route through `capture_mechanism`,
/// so `@e` yields `{ $tag, $data }` instead of a bare node.
#[test]
fn tagged_alt_under_node_capture_emits_union() {
    shot_exec!(
        r#"Q = (program (expression_statement [A: (identifier) @a  B: (number) @b] @e))"#,
        "foo"
    );
}

/// #420 #4: an uncaptured recursive reference is an opaque boundary. Inference
/// types it `Void`; the VM now suppresses the captures inside the recursion
/// instead of bubbling them, so the outer `name` stays `null` and the value
/// matches its declared type.
#[test]
fn uncaptured_recursive_ref_suppresses_captures() {
    shot_exec!(
        "Nested = (call_expression function: [(identifier) @name (Nested)])\nQ = (program (expression_statement (Nested) @c))",
        "a()()"
    );
}

/// #420 #5: a `:: string` annotation on an array capture preserves the array
/// shape (it recurses into the element) instead of discarding it and panicking.
#[test]
fn string_annotation_on_array_capture() {
    shot_exec!(
        r#"Q = (program (lexical_declaration (variable_declarator (identifier))+ @ids :: string))"#,
        "let a, b;"
    );
}

/// #420 #6: `field: (Def) @cap` nests the referenced definition under the
/// capture. Inference and emission agree on the nested `{ fn: Name }` shape
/// rather than flattening `Name`'s fields into the parent.
#[test]
fn field_def_capture_nests() {
    shot_exec!(
        "Name = (identifier) @text\nQ = (program (function_declaration name: (Name) @fn))",
        "function foo(){}"
    );
}

/// #420 #7: an absent optional scalar materializes as `null` (declared
/// `Node | null`, always present), never an absent key.
#[test]
fn optional_scalar_absent_is_null() {
    shot_exec!(
        r#"Q = (program (lexical_declaration)? @decl (expression_statement) @stmt)"#,
        "foo;"
    );
}

/// #420 #7: a list capture absent from the matched alternation branch
/// materializes as `[]` (declared `Node[]`), never `null`.
#[test]
fn array_capture_absent_in_branch_is_empty() {
    shot_exec!(
        r#"Q = (program [(lexical_declaration (variable_declarator)+ @vs)  (expression_statement) @s])"#,
        "foo;"
    );
}

/// #420 #7: an *optional* list (`((x)+ @a)?`) stays nullable across branches. The
/// absent `?` emits `null`, so relaxing the field by its array *shape* — instead
/// of its nullability — would force a non-null `[]` and make the type lie. On `f()`
/// the empty argument list skips the `?`, so `args` is `null`, matching the
/// declared `[Node, ...Node[]] | null`.
#[test]
fn optional_array_field_stays_nullable_across_branches() {
    shot_exec!(
        r#"Q = (program (expression_statement [(call_expression (arguments (identifier)+ @args)?)  (identifier) @id]))"#,
        "f()"
    );
}

/// #420 #7: an untagged branch contributes only its *top-level* fields. Branch two
/// supplies `row`; its nested `@x` belongs to that scope, so the top-level `x`
/// (from branch one) must still materialize as `null`. The default is injected on
/// the parent object and has to survive the `Obj` the branch body opens for `row`.
#[test]
fn alt_branch_nested_scope_capture_keeps_top_level_null() {
    shot_exec!(
        r#"Q = (program (expression_statement [(number) @x  {(identifier) @x} @row]))"#,
        "foo"
    );
}

/// #420 #8: a reference into another workspace file is validated and inferred
/// under its *own* source. The duplicate capture lives in `idents.ptk`, so the
/// diagnostic points there — previously the referrer's source id was reused,
/// slicing foreign content out of bounds (a panic).
#[test]
fn cross_file_ref_attributes_diagnostic_to_owning_file() {
    shot_workspace_check!(&[
        (
            "main.ptk",
            "Main = (program (expression_statement (Idents)))"
        ),
        (
            "idents.ptk",
            "Idents = (binary_expression (identifier) @x (identifier) @x)"
        ),
    ]);
}

// IR pass soundness (#421).

/// Regression for #421: `collapse_prefix` deleted a still-referenced instruction
/// for back-to-back optionals, so `check` passed but `emit`/`dump`/`run` panicked
/// with "label not in layout". Deleting the pass (an unsound size optimization)
/// fixes it; this query must compile and execute through the seam.
#[test]
fn collapse_prefix_drops_referenced_instruction() {
    shot_exec!(r#"Q = (program (comment)? (comment)? (comment)?)"#, "// c");
}

// #421 scope balance: a capture's enclosing-scope effects — a tagged variant's
// `Enum`/`EndEnum`, an untagged branch's null-injected defaults — must stay
// balanced along every path, regardless of the capture mechanism or a leading
// skippable item. These shapes used to drop an `Enum`-open or duplicate it.

/// Regression for #421 (bug 4): a tagged-alternation variant whose body is a
/// sequence with a leading optional. When the optional is skipped, `Enum` (opened
/// only on the present branch) and the trailing `EndEnum` left the path unbalanced,
/// and the follower over-advanced past the first child. The skip path must open the
/// variant scope once and bind `a`/`b` to the first two children.
#[test]
fn tagged_variant_leading_optional_skipped() {
    shot_exec!(
        r#"Q = (program [Tag: {(comment)? (expression_statement) @a (expression_statement) @b}])"#,
        "a; b;"
    );
}

/// Companion to `tagged_variant_leading_optional_skipped`: the present-branch path
/// (optional matched) must yield the same `Tag` shape, with the scope opened once.
#[test]
fn tagged_variant_leading_optional_matched() {
    shot_exec!(
        r#"Q = (program [Tag: {(comment)? (expression_statement) @a (expression_statement) @b}])"#,
        "/* c */ a; b;"
    );
}

/// Regression for #421 (bug 4): the skipped optional carries its own capture `c`.
/// The `Null Set` for `c` must land inside the variant scope (so `c: null` sits in
/// `$data`, not outside the tagged value), and `a`/`b` must still bind.
#[test]
fn tagged_variant_leading_optional_nulls_inner_capture() {
    shot_exec!(
        r#"Q = (program [Tag: {(expression_statement (call_expression) @c)? (expression_statement) @a (expression_statement) @b}])"#,
        "a; b;"
    );
}

/// Regression for #421 (bug 4): an array of tagged variants whose body has a
/// skippable boundary. The element's post-effects are `[EndEnum, Push]` — the close
/// produces the variant value, `Push` consumes it into the array. They must stay
/// together on the path that closes the scope: splitting them would push before the
/// close (panic) or, on the skip path, never push (empty array). Each statement must
/// appear as its own `Tag` element.
#[test]
fn tagged_variant_array_with_skippable_boundary() {
    shot_exec!(
        r#"Q = (program ([Tag: {(expression_statement) @a (comment)?}])* @items)"#,
        "a; b;"
    );
}

/// Captured group `((…) @a) @x` as a tagged variant: the variant's `Enum` wraps
/// the capture's own `Obj` scope, so `x` holds the struct `{a}` inside the `Tag`
/// value. Were the `Enum`-open (the capture's enclosing `pre`) dropped, the path
/// would close an `EndEnum` with no matching open.
#[test]
fn tagged_variant_captured_group_struct() {
    shot_exec!(
        r#"Q = (program [Tag: ((expression_statement) @a) @x])"#,
        "a;"
    );
}

/// Suppressive capture `(…) @_` as a tagged variant: the `Enum` opens before
/// `SuppressBegin`, not at the `SuppressEnd` exit, so the scope stays balanced.
/// The payload is suppressed, so the result is the bare `Tag` with no `$data`.
#[test]
fn tagged_variant_captured_suppressed() {
    shot_exec!(r#"Q = (program [Tag: (expression_statement) @_])"#, "a;");
}

/// A captured tagged enum `[In: …] @y` as a tagged variant: the outer `Enum` opens
/// before the inner enum runs, so the outer `EndEnum` has a match and the two enums
/// nest — `y` holds the `In` value inside the `Tag` value.
#[test]
fn tagged_variant_captured_inner_enum() {
    shot_exec!(
        r#"Q = (program [Tag: [In: (expression_statement) @a] @y])"#,
        "a;"
    );
}

/// The untagged, silent twin: a branch nulls the captures it lacks, and that
/// `Null Set` rides the capture's enclosing `pre`. With a captured-group branch,
/// `b` must come back `null` (not missing) in the merged result — a dropped
/// pre-effect here is wrong output with no panic.
#[test]
fn untagged_alt_captured_group_branch_nulls_sibling() {
    shot_exec!(
        r#"Q = (program [(((expression_statement) @a) @grp) ((comment) @b)] @w)"#,
        "a;"
    );
}

/// #383: a captured ref whose callee returns via a non-greedy optional skip
/// matched nothing, so the required `@x` cannot bind — the query yields no
/// match instead of fabricating the call-site (`program`) node.
#[test]
fn captured_ref_via_non_greedy_optional_skip() {
    shot_exec!("A = (identifier)??\nQ = (A) @x", "foo");
}

/// #383 (descend flavor): the same zero-width return reached through a `Down`
/// call captures the descended child (`expression_statement`) unless the empty
/// match is rejected. Expected: no match.
#[test]
fn captured_ref_via_optional_skip_through_descent() {
    shot_exec!("A = (identifier)??\nQ = (program (A) @x)", "foo");
}

/// #383 (ascension flavor): a captured ref that matches a child and then
/// ascends must capture the matched child, not the parent it ascends to. The
/// capture used to ride the closing `Up` step and grab `program`; it now binds
/// `lexical_declaration`.
#[test]
fn captured_ref_keeps_match_across_ascension() {
    shot_exec!("A = (lexical_declaration)\nQ = (program (A) @a)", "let x;");
}

/// #419: an anchor-preceded ref is wrapped in the unified position search, so a
/// failed candidate must drive a sibling retry through the wrapper. The first
/// comma's anchored sibling is a number (fail); the search advances to the
/// second comma, whose sibling is the string. Exercises ref-body backtracking
/// at runtime, beyond the emit snapshot.
#[test]
fn ref_before_anchor_backtracks_across_siblings() {
    shot_exec!(
        "Comma = \",\"\nQ = (program (expression_statement (array (Comma) . (string) @s)))",
        r#"[1, 2, "x"]"#
    );
}

/// #441 regression: an anchor before a quantified follower is enforced. `a` is
/// skipped, so the leading anchor pins `b` to the first child; `debugger` sits
/// after `bar`, so the match is correctly rejected.
#[test]
fn anchored_quantified_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

/// #441: the same overshoot for `+`, `?`, and the non-greedy `*?`/`+?` — every
/// quantifier kind honors the leading anchor after the optional is skipped.
#[test]
fn anchored_plus_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)+ @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

#[test]
fn anchored_optional_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)? @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

#[test]
fn anchored_nongreedy_star_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)*? @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

#[test]
fn anchored_nongreedy_plus_follower_overshoots() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)+? @b . (expression_statement (identifier == "foo")) @c})"#,
        "bar; debugger; foo;"
    );
}

/// #441 match path: optional present, the `*` follower starts adjacent to it and
/// collects the back-to-back run of debuggers before `foo`.
#[test]
fn anchored_star_follower_match_path() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "let x; debugger; debugger; foo;"
    );
}

/// #441 skip path: optional absent, the `*` follower starts at the first child
/// and collects the leading run of debuggers → match with `a` null.
#[test]
fn anchored_star_follower_skip_path_match() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "debugger; debugger; foo;"
    );
}

/// #441: a soft anchor before the quantified follower still skips a leading
/// comment on the skip path → match.
#[test]
fn anchored_soft_quantified_follower_skips_comment() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "/* c */ debugger; foo;"
    );
}

/// #441: a strict anchor before the quantified follower rejects the leading
/// comment → no match.
#[test]
fn anchored_strict_quantified_follower_rejects_comment() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a .! (debugger_statement)* @b . (expression_statement (identifier == "foo")) @c})"#,
        "/* c */ debugger; foo;"
    );
}

/// #439 regression: a trailing anchor no longer suppresses the interior anchor
/// after a skippable first item. `a` is skipped, so `b` must be the first child;
/// `debugger` is the second, so the match is correctly rejected.
#[test]
fn interior_anchor_ignored_with_trailing_anchor() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement) @b .})"#,
        "foo; debugger;"
    );
}

/// #439 match path: optional present and adjacent, follower is the last child.
#[test]
fn trailing_anchor_optional_present_adjacent() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement) @b .})"#,
        "let x; debugger;"
    );
}

/// #439 match path: optional present but a statement breaks adjacency → no match.
#[test]
fn trailing_anchor_optional_present_adjacency_broken() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement) @b .})"#,
        "let x; foo; debugger;"
    );
}

/// #439 skip path: optional absent, follower is the sole child (first and last)
/// → match with `a` null.
#[test]
fn trailing_anchor_optional_absent_sole_child() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement) @b .})"#,
        "debugger;"
    );
}

/// #439 strict variants: `.!` on both ends. Optional absent, follower is the
/// sole child → match; a leading comment breaks the strict leading anchor.
#[test]
fn trailing_strict_anchor_optional_absent_sole_child() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a .! (debugger_statement) @b .!})"#,
        "debugger;"
    );
}

#[test]
fn trailing_strict_anchor_optional_absent_rejects_leading_comment() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a .! (debugger_statement) @b .!})"#,
        "/* c */ debugger;"
    );
}

/// #439 strict variant, match path: present and strictly adjacent → match;
/// a comment between the pair breaks strict adjacency → no match.
#[test]
fn trailing_strict_anchor_optional_present_adjacent() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a .! (debugger_statement) @b .!})"#,
        "let x; debugger;"
    );
}

#[test]
fn trailing_strict_anchor_optional_present_comment_breaks_adjacency() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a .! (debugger_statement) @b .!})"#,
        "let x; /* c */ debugger;"
    );
}

/// #439 × #441: optional first item, a quantified follower, AND a trailing
/// anchor at once. With `@a` skipped the follower starts at the first child,
/// collects the back-to-back run, and the run's end must be the last child.
#[test]
fn trailing_anchor_quantified_follower_skip_path_match() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b .})"#,
        "debugger; debugger;"
    );
}

#[test]
fn trailing_anchor_quantified_follower_not_last() {
    shot_exec!(
        r#"Q = (program {(lexical_declaration)? @a . (debugger_statement)* @b .})"#,
        "debugger; foo;"
    );
}

/// BUG #417: a supertype pattern (`statement` groups concrete kinds and is never
/// itself produced by tree-sitter) silently never matches instead of being
/// rejected at link time, yielding an empty array.
#[test]
fn supertype_pattern_silently_never_matches() {
    shot_exec!(r#"Q = (program (statement)* @s)"#, "let x;");
}

// Regex predicate execution (#426). The DFAs are deserialized once at module
// load and reused on every evaluation; these pin the match/no-match behavior
// that the load-time cache must preserve, including repeated evaluation of a
// single cached DFA across a quantified pattern. Inputs put the needle off the
// start (`barfoo`, `ax`) so the `^` anchor is genuinely exercised — an
// unanchored search would (wrongly) match these.

/// A `=~` predicate that matches binds its node.
#[test]
fn regex_predicate_matches() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier =~ /^foo/) @id))"#,
        "foobar;"
    );
}

/// A `=~` predicate that fails gates the whole match out. `barfoo` contains the
/// needle but not at the start, so the `^` anchor must reject it.
#[test]
fn regex_predicate_rejects_non_match() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier =~ /^foo/) @id))"#,
        "barfoo;"
    );
}

/// A negated `!~` predicate passes exactly when the pattern does not match;
/// `^foo` does not match `barfoo`, so the node binds.
#[test]
fn negated_regex_predicate_matches_non_match() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier !~ /^foo/) @id))"#,
        "barfoo;"
    );
}

/// One cached DFA, evaluated once per statement across a quantified pattern —
/// the hot path the load-time cache exists to serve (#426). `ax` carries the
/// needle off the start, so only the anchored `x1`/`x3` survive.
#[test]
fn regex_predicate_in_quantified_pattern() {
    shot_exec!(
        r#"Q = (program (expression_statement (identifier =~ /^x/) @id)* @rows)"#,
        "x1; ax; x3;"
    );
}
