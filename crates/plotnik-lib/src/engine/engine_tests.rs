//! VM execution tests with snapshot-based verification.
//!
//! Tests are organized by feature and use file-based snapshots.
//! Each test captures query, source, and JSON output.

use indoc::indoc;

use crate::QueryBuilder;
use crate::bytecode::Module;
use crate::emit::emit_linked;

use super::{Materializer, VM, ValueMaterializer};

/// Execute a query against source code and return the JSON output.
fn execute(query: &str, source: &str) -> String {
    execute_with_entry(query, source, None)
}

/// Execute a query against source code with a specific entrypoint.
fn execute_with_entry(query: &str, source: &str, entry: Option<&str>) -> String {
    let lang = plotnik_langs::javascript();

    let query_obj = QueryBuilder::one_liner(query)
        .parse()
        .expect("parse failed")
        .analyze()
        .link(&lang);

    assert!(query_obj.is_valid(), "query should be valid");

    let bytecode = emit_linked(&query_obj).expect("emit failed");
    let module = Module::load(&bytecode).expect("decode failed");

    let tree = lang.parse(source);
    let trivia = build_trivia_types(&module);
    let vm = VM::builder(source, &tree).trivia_types(trivia).build();

    let entrypoint = resolve_entrypoint(&module, entry);
    let effects = vm
        .execute(&module, 0, &entrypoint)
        .expect("execution failed");

    let materializer = ValueMaterializer::new(source, module.types(), module.strings());
    let value = materializer.materialize(effects.as_slice(), entrypoint.result_type);

    serde_json::to_string_pretty(&value).expect("json serialization failed")
}

/// Build list of trivia node type IDs from module metadata.
fn build_trivia_types(module: &Module) -> Vec<u16> {
    let node_types = module.node_types();
    let strings = module.strings();
    let mut trivia = Vec::new();
    for i in 0..node_types.len() {
        let t = node_types.get(i);
        let name = strings.get(t.name);
        if name == "comment" {
            trivia.push(t.id);
        }
    }
    trivia
}

/// Resolve entrypoint by name or use the default.
fn resolve_entrypoint(module: &Module, name: Option<&str>) -> crate::bytecode::Entrypoint {
    let entrypoints = module.entrypoints();
    let strings = module.strings();

    if let Some(name) = name {
        for i in 0..entrypoints.len() {
            let e = entrypoints.get(i);
            if strings.get(e.name) == name {
                return e;
            }
        }
        panic!("entrypoint not found: {}", name);
    }

    // Default: first entrypoint
    entrypoints.get(0)
}

macro_rules! snap {
    ($query:expr, $source:expr) => {{
        let query = $query.trim();
        let source = $source.trim();
        let output = execute(query, source);
        insta::with_settings!({
            omit_expression => true
        }, {
            insta::assert_snapshot!(format!("{query}\n---\n{source}\n---\n{output}"));
        });
    }};
    ($query:expr, $source:expr, entry: $entry:expr) => {{
        let query = $query.trim();
        let source = $source.trim();
        let output = execute_with_entry(query, source, Some($entry));
        insta::with_settings!({
            omit_expression => true
        }, {
            insta::assert_snapshot!(format!("{query}\n---\n{source}\n---\n{output}"));
        });
    }};
}

// Simple captures

#[test]
fn capture_single() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name)))",
        "let x = 1"
    );
}

#[test]
fn capture_multiple() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name value: (number) @value)))",
        "let x = 42"
    );
}

#[test]
fn capture_string_annotation() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name :: string)))",
        "let myVar = 1"
    );
}

// Quantifiers

#[test]
fn quantifier_star() {
    snap!(
        "Q = (program (expression_statement (array (number)* @nums)))",
        "[1, 2, 3]"
    );
}

#[test]
fn quantifier_plus() {
    snap!(
        "Q = (program (expression_statement (array (number)+ @nums)))",
        "[1, 2, 3]"
    );
}

#[test]
fn quantifier_optional_present() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name value: (number)? @value)))",
        "let x = 42"
    );
}

#[test]
fn quantifier_optional_absent() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name value: (number)? @value)))",
        "let x"
    );
}

#[test]
fn quantifier_nongreedy_star() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator)*? @decls))",
        "let a, b"
    );
}

#[test]
fn quantifier_struct_array() {
    snap!(
        "Q = (program (lexical_declaration {(variable_declarator name: (identifier) @name) @decl}* @decls))",
        "let a, b, c"
    );
}

/// Regression: string annotation on array capture should extract text.
/// `(identifier)* @names :: string` should produce string[], not Node[].
#[test]
fn quantifier_star_with_string_annotation() {
    snap!(
        "Q = (program (expression_statement (array (identifier)* @names :: string)))",
        "[a, b, c]"
    );
}

/// Regression: string annotation on non-empty array capture.
/// `(identifier)+ @names :: string` should produce [string, ...string[]].
#[test]
fn quantifier_plus_with_string_annotation() {
    snap!(
        "Q = (program (expression_statement (array (identifier)+ @names :: string)))",
        "[a, b, c]"
    );
}

// Alternations

#[test]
fn alternation_tagged_num() {
    snap!(
        indoc! {r#"
            Value = [Ident: (identifier) @name  Num: (number) @val]
            Q = (program (lexical_declaration (variable_declarator value: (Value) @value)))
        "#},
        "let x = 42",
        entry: "Q"
    );
}

#[test]
fn alternation_tagged_ident() {
    snap!(
        indoc! {r#"
            Value = [Ident: (identifier) @name  Num: (number) @val]
            Q = (program (lexical_declaration (variable_declarator value: (Value) @value)))
        "#},
        "let x = y",
        entry: "Q"
    );
}

/// Regression: tagged alternation with named definition reference.
/// When a definition is parsed before the alternation, Symbol interning order
/// differs from AST branch order. The variant index must use BTreeMap order
/// (by Symbol), not AST iteration order.
#[test]
fn alternation_tagged_definition_ref_backtrack() {
    snap!(
        indoc! {r#"
            Block = (call_expression function: (identifier) @name)
            Statement = [Assign: (assignment_expression) @a  Block: (Block) @b]
            Q = (program (expression_statement (Statement) @stmt))
        "#},
        "foo()",
        entry: "Q"
    );
}

#[test]
fn alternation_merge_num() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator value: [(identifier) @ident (number) @num])))",
        "let x = 42"
    );
}

#[test]
fn alternation_merge_ident() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator value: [(identifier) @ident (number) @num])))",
        "let x = y"
    );
}

// Recursion

#[test]
fn recursion_member_chain() {
    snap!(
        indoc! {r#"
            Chain = [Base: (identifier) @name  Access: (member_expression object: (Chain) @base property: (property_identifier) @prop)]
            Q = (program (expression_statement (Chain) @chain))
        "#},
        "a.b.c",
        entry: "Q"
    );
}

#[test]
fn recursion_nested_calls() {
    snap!(
        indoc! {r#"
            Main = (program (expression_statement (Call)))
            Call = (call_expression function: (identifier) @name arguments: (arguments (Call)? @inner))
        "#},
        "foo(bar())",
        entry: "Main"
    );
}

// Anchors

#[test]
fn anchor_first_child() {
    snap!(
        "Q = (program (lexical_declaration . (variable_declarator) @first))",
        "let a, b, c"
    );
}

#[test]
fn anchor_adjacency() {
    snap!(
        "Q = (program (lexical_declaration {(variable_declarator) @first . (variable_declarator) @second}))",
        "let a, b, c"
    );
}

/// Trailing anchor requires backtracking when first match isn't last.
/// The skip-retry pattern retries with next sibling until finding the last one.
#[test]
fn anchor_last_child() {
    snap!(
        "Q = (program (function_declaration) @last .)",
        "function first() {} function second() {}"
    );
}

/// Trailing anchor with only one sibling - trivial case.
#[test]
fn anchor_last_child_single() {
    snap!(
        "Q = (program (function_declaration) @only .)",
        "function only() {}"
    );
}

/// Trailing anchor skips trivia (comments) when checking last position.
#[test]
fn anchor_last_child_with_trivia() {
    snap!(
        "Q = (program (function_declaration) @last .)",
        "function first() {} function second() {} /* trailing comment */"
    );
}

/// Multi-item sequence with trailing anchor.
/// Should find (b, c) not (a, b) because c must be last.
#[test]
fn anchor_last_child_multi_item() {
    snap!(
        "Q = (program {(function_declaration) @a (function_declaration) @b .})",
        "function a() {} function b() {} function c() {}"
    );
}

// Fields

#[test]
fn field_negated_absent() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name !value)))",
        "let x"
    );
}

// Search behavior

#[test]
fn search_skip_siblings() {
    snap!(
        "Q = (program (statement_block (return_statement) @ret))",
        "{ foo(); bar(); return 1; }"
    );
}

// Regression tests

/// BUG #1: Scalar node arrays produced null values.
/// The issue was that `(identifier)* @args` captured an array of nulls instead
/// of actual node values because [Node, Push] effects were missing.
#[test]
fn regression_scalar_array_captures_nodes() {
    snap!(
        indoc! {r#"
            Q = (program (expression_statement (call_expression
                function: (identifier) @fn
                arguments: (arguments (identifier)* @args))))
        "#},
        "foo(a, b, c)"
    );
}

/// BUG #2: Tagged alternations panicked with "type member index out of bounds".
/// The issue was that enum types from tagged alternations weren't being collected
/// for bytecode emission when inside named nodes that don't propagate TypeFlow::Scalar.
#[test]
fn regression_tagged_alternation_materializes() {
    snap!(
        indoc! {r#"
            Q = (program (expression_statement [
                Ident: (identifier) @x
                Num: (number) @y
            ]))
        "#},
        "42"
    );
}

/// BUG #3: Recursive patterns produced invalid JSON with duplicate keys.
/// The issue was that Call/Return didn't create proper Obj/EndObj scopes, so
/// recursive calls would flatten their captures into the same scope as the caller.
#[test]
fn regression_recursive_captures_nest_properly() {
    snap!(
        indoc! {r#"
            Main = (program (expression_statement (Call)))
            Call = (call_expression
                function: (identifier) @name
                arguments: (arguments (Call)? @inner))
        "#},
        "foo(bar())",
        entry: "Main"
    );
}

/// BUG #4: Call instructions didn't search for field constraints.
/// When a Call instruction has navigation with a field constraint, it should
/// continue searching (according to skip policy) until finding a node with the
/// required field, not immediately backtrack on the first mismatch.
#[test]
fn regression_call_searches_for_field_constraint() {
    snap!(
        indoc! {r#"
            Expr = [Lit: (number) @val  Binary: (binary_expression left: (Expr) @left right: (Expr) @right)]
            Q = (program (expression_statement (Expr) @expr))
        "#},
        "1 + 2",
        entry: "Q"
    );
}

/// BUG #5: Named definitions didn't search among siblings like inline patterns.
/// When using a named definition like `(GString)` inside a parent pattern, it
/// should search among siblings the same way an inline pattern like `(string)`
/// would. This is the "safe refactoring" promise: extracting a pattern to a
/// named definition should not change matching behavior.
#[test]
fn regression_call_searches_among_siblings() {
    snap!(
        indoc! {r#"
            Num = (number) @n
            Q = (program (expression_statement (array (Num) @num)))
        "#},
        "[1, 2, 3]",
        entry: "Q"
    );
}

/// BUG #6: Tagged alternation used wrong $tag when branches reference named definitions.
/// The issue was that enum members are emitted in BTreeMap (Symbol interning) order,
/// but the compiler used AST branch order for member indices. When a definition name
/// (e.g., "Second") is interned before a branch label (e.g., "First"), the indices mismatch.
#[test]
fn regression_enum_tag_with_definition_refs() {
    // "Second" is interned as a definition name before "First" is seen as a branch label.
    // This caused the First branch to incorrectly get tagged as "Second".
    snap!(
        indoc! {r#"
            Second = (binary_expression left: (number) @left right: (number) @right)
            Q = (program (expression_statement [First: (number) @num  Second: (Second) @bin]))
        "#},
        "42",
        entry: "Q"
    );
}

// Suppressive captures

/// Suppressive capture (@_) matches structurally but doesn't produce output.
#[test]
fn suppressive_capture_anonymous() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @_)))",
        "let x = 1"
    );
}

/// Named suppressive capture (@_name) also suppresses output.
#[test]
fn suppressive_capture_named() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @_name value: (number) @value)))",
        "let x = 42"
    );
}

/// Suppressive capture with sibling regular capture - only regular capture appears.
#[test]
fn suppressive_capture_with_regular_sibling() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @_ value: (number) @value)))",
        "let x = 42"
    );
}

/// Suppressive capture suppresses inner captures from definitions.
#[test]
fn suppressive_capture_suppresses_inner() {
    snap!(
        indoc! {r#"
            Expr = (binary_expression left: (number) @left right: (number) @right)
            Q = (program (expression_statement (Expr) @_))
        "#},
        "1 + 2",
        entry: "Q"
    );
}

/// Suppressive capture with wrap pattern: { inner } @_ wraps and suppresses.
#[test]
fn suppressive_capture_with_wrap() {
    snap!(
        indoc! {r#"
            Expr = (binary_expression left: (number) @left right: (number) @right)
            Q = (program (expression_statement {(Expr) @_} @expr))
        "#},
        "1 + 2",
        entry: "Q"
    );
}

/// Named wildcard `(_)` matches only named nodes, skipping anonymous nodes.
/// In `return 42`, `(_)` should match `number`, not the `return` keyword.
#[test]
fn wildcard_named_skips_anonymous() {
    snap!("Q = (program (return_statement (_) @x))", "return 42");
}

/// Bare wildcard `_` matches any node including anonymous ones.
/// In `return 42`, `_` should match the first child which is `return` keyword.
#[test]
fn wildcard_bare_matches_anonymous() {
    snap!("Q = (program (return_statement _ @x))", "return 42");
}

/// BUG: Unlabeled alternation with 5+ branches and unique captures per branch.
///
/// Each branch in an unlabeled alternation needs to inject null for captures
/// it doesn't have. With 5 branches (@a, @b, @c, @d, @e), each branch needs
/// 8 pre-effects (4 nulls + 4 sets). This exceeds the 3-bit limit (max 7)
/// and caused bytecode encoding overflow, resulting in garbage addresses.
///
/// Fix: Cascade overflow pre-effects into leading epsilon transitions.
#[test]
fn regression_unlabeled_alternation_5_branches_captures() {
    snap!(
        indoc! {r#"
            Q = (program (expression_statement [
                (identifier) @a
                (number) @b
                (string) @c
                (binary_expression) @d
                (call_expression) @e
            ]))
        "#},
        "foo"
    );
}

/// Same bug with 8 branches - even more pre-effects requiring multiple cascade steps.
#[test]
fn regression_unlabeled_alternation_8_branches_captures() {
    snap!(
        indoc! {r#"
            Q = (program (expression_statement [
                (identifier) @a
                (number) @b
                (string) @c
                (binary_expression) @d
                (call_expression) @e
                (member_expression) @f
                (array) @g
                (object) @h
            ]))
        "#},
        "42"
    );
}

/// BUG #7: Node capture on named node with children captured wrong target.
///
/// For `(class_declaration body: (class_body ...)) @class`, the @class capture
/// was capturing `class_body` instead of `class_declaration`. This happened because
/// Node/Text capture effects were placed on the exit (after children processing),
/// but matched_node gets clobbered by child matches and backtracking.
///
/// Fix: Split Node/Text effects to entry match's post_effects, where matched_node
/// is still valid from the parent match.
#[test]
fn regression_node_capture_on_parent_with_children() {
    snap!(
        indoc! {r#"
            Q = (program (class_declaration
                body: (class_body (method_definition)* @methods)
            ) @class)
        "#},
        "class A { foo() {} }"
    );
}

/// BUG #8: Nested quantifiers with empty inner array found only first outer match.
///
/// For `{ (class_declaration ...) @class }* @classes` with multiple classes where
/// some have empty method arrays, only the first class was captured. The skip path
/// (zero matches on inner array) was bypassing an Up navigation, leaving the cursor
/// at the wrong level for the outer loop.
///
/// Fix: Remove skip_exit bypass - all paths through children must go through Up,
/// because even "skip" paths descend to check for matches before backtracking.
#[test]
fn regression_nested_quantifiers_empty_inner_array() {
    snap!(
        indoc! {r#"
            Q = (program {(class_declaration
                body: (class_body (method_definition)* @methods)
            ) @class}* @classes)
        "#},
        "class A { } class B { foo() {} }"
    );
}

/// Combined regression test: nested quantifiers with struct captures and mixed empty/non-empty.
///
/// This exercises both bugs together: Node capture must capture class_declaration (not class_body),
/// and all classes must be found regardless of whether their method arrays are empty.
#[test]
fn regression_nested_quantifiers_struct_captures_mixed() {
    snap!(
        indoc! {r#"
            Q = (program {(class_declaration
                name: (identifier) @name
                body: (class_body {(method_definition
                    name: (property_identifier) @method_name
                ) @method}* @methods)
            ) @class}* @classes)
        "#},
        "class Empty { } class One { foo() {} } class Two { bar() {} baz() {} }"
    );
}

/// BUG #9: Childless nodes with inner quantifiers caused outer quantifier to miss matches.
///
/// For `(identifier (_)* @items)` inside a star quantifier, if `identifier` has zero
/// children at runtime (tree-sitter identifiers are terminal nodes with no children),
/// the Down navigation fails. The skip path should NOT execute Up since we never
/// descended, but currently it does, causing the cursor to ascend one level too high.
///
/// This breaks the outer quantifier's iteration: after processing the first match,
/// the cursor is at the grandparent level instead of sibling level, so Next fails.
///
/// Fix: When Down fails immediately (childless node), the skip path should bypass
/// the Up instruction. Only execute Up when we actually descended into children.
#[test]
fn regression_childless_node_with_inner_star() {
    snap!(
        indoc! {r#"
            Q = (program {(expression_statement
                (identifier (_)* @items) @id
            ) @stmt}* @stmts)
        "#},
        "foo; bar; baz"
    );
}

/// Same bug with optional quantifier instead of star.
/// `(identifier (_)? @item)` should also handle childless nodes correctly.
#[test]
fn regression_childless_node_with_inner_optional() {
    snap!(
        indoc! {r#"
            Q = (program {(expression_statement
                (identifier (_)? @item) @id
            ) @stmt}* @stmts)
        "#},
        "foo; bar; baz"
    );
}

/// Regression test: recursive labeled alternation with uncaptured star quantifier
/// should produce arrays, not nested tagged unions.
///
/// When `(RefName)*` is inside a labeled alternation variant (like Descend),
/// and RefName returns an enum type, the quantifier needs to produce an array
/// even without an explicit `@capture` annotation.
#[test]
fn regression_recursive_alternation_uncaptured_array() {
    snap!(
        indoc! {r#"
            E = [
                A: (identifier) @x
                B: (_ (E)*)
            ]
            Test = (program (E)* @items)
        "#},
        "x; y;",
        entry: "Test"
    );
}

/// Regression test: labeled alternation variant with array capture payload.
///
/// When a labeled alternation variant has an array capture as its payload
/// (e.g., `[First: (E)* @arr]`), the Enum effect must be emitted BEFORE the Arr
/// effect. Previously, `compile_array_scope` forgot to emit `outer_capture.pre`,
/// causing a stack mismatch panic in the materializer.
#[test]
fn regression_enum_variant_array_capture_payload() {
    snap!(
        indoc! {r#"
            E = [A: (identifier) @x  B: (number) @y]
            F = [First: (E)* @arr  Second: (E)+ @arr2]
            Test = (program (expression_statement (F)))
        "#},
        "x",
        entry: "Test"
    );
}
