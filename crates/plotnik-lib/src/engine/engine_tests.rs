//! VM execution tests with snapshot-based verification.
//!
//! Tests are organized by feature and use file-based snapshots.
//! Each test captures query, source, and JSON output.

use indoc::indoc;

use crate::bytecode::Module;
use crate::emit::emit_linked;
use crate::QueryBuilder;

use super::{FuelLimits, Materializer, ValueMaterializer, VM};

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
    let module = Module::from_bytes(bytecode).expect("decode failed");

    let tree = lang.parse(source);
    let trivia = build_trivia_types(&module);
    let vm = VM::new(&tree, trivia, FuelLimits::default());

    let entrypoint = resolve_entrypoint(&module, entry);
    let effects = vm.execute(&module, &entrypoint).expect("execution failed");

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
fn resolve_entrypoint(
    module: &Module,
    name: Option<&str>,
) -> crate::bytecode::Entrypoint {
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

// ============================================================================
// 1. SIMPLE CAPTURES
// ============================================================================

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

// ============================================================================
// 2. QUANTIFIERS
// ============================================================================

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

// ============================================================================
// 3. ALTERNATIONS
// ============================================================================

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

// ============================================================================
// 4. RECURSION
// ============================================================================

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

// ============================================================================
// 5. ANCHORS
// ============================================================================

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

// ============================================================================
// 6. FIELDS
// ============================================================================

#[test]
fn field_negated_absent() {
    snap!(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name !value)))",
        "let x"
    );
}

// ============================================================================
// 7. SEARCH BEHAVIOR
// ============================================================================

#[test]
fn search_skip_siblings() {
    snap!(
        "Q = (program (statement_block (return_statement) @ret))",
        "{ foo(); bar(); return 1; }"
    );
}

// ============================================================================
// 8. REGRESSION TESTS
// ============================================================================

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
