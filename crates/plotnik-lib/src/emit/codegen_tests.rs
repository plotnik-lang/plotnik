//! Comprehensive bytecode emission tests.
//!
//! Tests are organized by language feature and use file-based snapshots.
//! Each test verifies the bytecode output for a specific language construct.

use crate::Query;
use indoc::indoc;

macro_rules! snap {
    ($query:expr) => {{
        let query = $query.trim();
        let bytecode = Query::expect_valid_bytecode(query);
        insta::with_settings!({
            omit_expression => true
        }, {
            insta::assert_snapshot!(format!("{query}\n---\n{bytecode}"));
        });
    }};
}

// ============================================================================
// 1. NODES
// ============================================================================

#[test]
fn nodes_named() {
    snap!(indoc! {r#"
        Test = (identifier) @id
    "#});
}

#[test]
fn nodes_anonymous() {
    snap!(indoc! {r#"
        Test = (binary_expression "+" @op)
    "#});
}

#[test]
fn nodes_wildcard_any() {
    snap!(indoc! {r#"
        Test = (pair key: _ @key)
    "#});
}

#[test]
fn nodes_wildcard_named() {
    snap!(indoc! {r#"
        Test = (pair key: (_) @key)
    "#});
}

#[test]
fn nodes_error() {
    snap!(indoc! {r#"
        Test = (ERROR) @err
    "#});
}

#[test]
fn nodes_missing() {
    snap!(indoc! {r#"
        Test = (MISSING) @m
    "#});
}

// ============================================================================
// 2. CAPTURES
// ============================================================================

#[test]
fn captures_basic() {
    snap!(indoc! {r#"
        Test = (identifier) @name
    "#});
}

#[test]
fn captures_multiple() {
    snap!(indoc! {r#"
        Test = (binary_expression (identifier) @a (number) @b)
    "#});
}

#[test]
fn captures_nested_flat() {
    snap!(indoc! {r#"
        Test = (a (b (c) @c) @b) @a
    "#});
}

#[test]
fn captures_deeply_nested() {
    snap!(indoc! {r#"
        Test = (a (b (c (d) @d) @c) @b) @a
    "#});
}

#[test]
fn captures_with_type_string() {
    snap!(indoc! {r#"
        Test = (identifier) @name :: string
    "#});
}

#[test]
fn captures_with_type_custom() {
    snap!(indoc! {r#"
        Test = (identifier) @name :: Identifier
    "#});
}

#[test]
fn captures_struct_scope() {
    snap!(indoc! {r#"
        Test = {(a) @a (b) @b} @item
    "#});
}

#[test]
fn captures_wrapper_struct() {
    snap!(indoc! {r#"
        Test = {{(identifier) @id (number) @num} @row}* @rows
    "#});
}

#[test]
fn captures_optional_wrapper_struct() {
    snap!(indoc! {r#"
        Test = {{(identifier) @id} @inner}? @outer
    "#});
}

// ============================================================================
// 3. FIELDS
// ============================================================================

#[test]
fn fields_single() {
    snap!(indoc! {r#"
        Test = (function_declaration name: (identifier) @name)
    "#});
}

#[test]
fn fields_multiple() {
    snap!(indoc! {r#"
        Test = (binary_expression
            left: (_) @left
            right: (_) @right)
    "#});
}

#[test]
fn fields_negated() {
    snap!(indoc! {r#"
        Test = (function_declaration name: (identifier) @name -type_parameters)
    "#});
}

#[test]
fn fields_alternation() {
    // Regression test: alternation in field position must have navigation on
    // the field-checking wrapper, not on the alternation branches.
    // See: wrapper navigates Down + checks field, branches use Stay.
    snap!(indoc! {r#"
        Test = (call_expression function: [(identifier) @fn (number) @num])
    "#});
}

// ============================================================================
// 4. QUANTIFIERS
// ============================================================================

#[test]
fn quantifiers_optional() {
    snap!(indoc! {r#"
        Test = (function_declaration (decorator)? @dec)
    "#});
}

#[test]
fn quantifiers_star() {
    snap!(indoc! {r#"
        Test = (identifier)* @items
    "#});
}

#[test]
fn quantifiers_plus() {
    snap!(indoc! {r#"
        Test = (identifier)+ @items
    "#});
}

#[test]
fn quantifiers_optional_nongreedy() {
    snap!(indoc! {r#"
        Test = (function_declaration (decorator)?? @dec)
    "#});
}

#[test]
fn quantifiers_star_nongreedy() {
    snap!(indoc! {r#"
        Test = (identifier)*? @items
    "#});
}

#[test]
fn quantifiers_plus_nongreedy() {
    snap!(indoc! {r#"
        Test = (identifier)+? @items
    "#});
}

#[test]
fn quantifiers_struct_array() {
    snap!(indoc! {r#"
        Test = (array {(identifier) @a (number) @b}* @items)
    "#});
}

#[test]
fn quantifiers_first_child_array() {
    snap!(indoc! {r#"
        Test = (array (identifier)* @ids (number) @n)
    "#});
}

#[test]
fn quantifiers_repeat_navigation() {
    snap!(indoc! {r#"
        Test = (function_declaration (decorator)* @decs)
    "#});
}

/// Regression test: sequence quantifiers in called definitions need sibling navigation.
/// Previously, `{...}*` compiled without navigation, causing infinite loops.
#[test]
fn quantifiers_sequence_in_called_def() {
    snap!(indoc! {r#"
        Item = (identifier) @name
        Collect = {(Item) @item}* @items
        Test = (parent (Collect))
    "#});
}

// ============================================================================
// 5. SEQUENCES
// ============================================================================

#[test]
fn sequences_basic() {
    snap!(indoc! {r#"
        Test = (parent {(a) (b)})
    "#});
}

#[test]
fn sequences_with_captures() {
    snap!(indoc! {r#"
        Test = (parent {(a) @a (b) @b})
    "#});
}

#[test]
fn sequences_nested() {
    snap!(indoc! {r#"
        Test = (parent {(a) {(b) (c)} (d)})
    "#});
}

#[test]
fn sequences_in_quantifier() {
    snap!(indoc! {r#"
        Test = (parent {(a) (b)}* @items)
    "#});
}

// ============================================================================
// 6. ALTERNATIONS
// ============================================================================

#[test]
fn alternations_unlabeled() {
    snap!(indoc! {r#"
        Test = [(identifier) @id (string) @str]
    "#});
}

#[test]
fn alternations_labeled() {
    snap!(indoc! {r#"
        Test = [
            A: (identifier) @a
            B: (number) @b
        ]
    "#});
}

#[test]
fn alternations_null_injection() {
    snap!(indoc! {r#"
        Test = [(identifier) @x (number) @y]
    "#});
}

#[test]
fn alternations_captured() {
    snap!(indoc! {r#"
        Test = [(identifier) (number)] @value
    "#});
}

#[test]
fn alternations_captured_tagged() {
    snap!(indoc! {r#"
        Test = [A: (identifier) @a  B: (number) @b] @item
    "#});
}

#[test]
fn alternations_in_quantifier() {
    snap!(indoc! {r#"
        Test = (object { [A: (pair) @a  B: (shorthand_property_identifier) @b] @item }* @items)
    "#});
}

#[test]
fn alternations_no_internal_captures() {
    snap!(indoc! {r#"
        Test = (program [(identifier) (number)] @x)
    "#});
}

// ============================================================================
// 7. ANCHORS
// ============================================================================

#[test]
fn anchors_between_siblings() {
    snap!(indoc! {r#"
        Test = (parent (a) . (b))
    "#});
}

#[test]
fn anchors_first_child() {
    snap!(indoc! {r#"
        Test = (parent . (first))
    "#});
}

#[test]
fn anchors_last_child() {
    snap!(indoc! {r#"
        Test = (parent (last) .)
    "#});
}

#[test]
fn anchors_with_anonymous() {
    snap!(indoc! {r#"
        Test = (parent "+" . (next))
    "#});
}

#[test]
fn anchors_no_anchor() {
    snap!(indoc! {r#"
        Test = (parent (a) (b))
    "#});
}

// ============================================================================
// 8. NAMED EXPRESSIONS
// ============================================================================

#[test]
fn definitions_single() {
    snap!(indoc! {r#"
        Foo = (identifier) @id
    "#});
}

#[test]
fn definitions_multiple() {
    snap!(indoc! {r#"
        Foo = (identifier) @id
        Bar = (string) @str
    "#});
}

#[test]
fn definitions_reference() {
    snap!(indoc! {r#"
        Expression = [(identifier) @name (number) @value]
        Root = (function_declaration name: (identifier) @name)
    "#});
}

#[test]
fn definitions_nested_capture() {
    snap!(indoc! {r#"
        Inner = (call (identifier) @name)
        Outer = (parent {(Inner) @item}* @items)
    "#});
}

// ============================================================================
// 9. RECURSION
// ============================================================================

#[test]
fn recursion_simple() {
    snap!(indoc! {r#"
        Expr = [
            Lit: (number) @value :: string
            Rec: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]
    "#});
}

#[test]
fn recursion_with_structured_result() {
    snap!(indoc! {r#"
        Expr = [
          Lit: (number) @value :: string
          Nested: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]

        Test = (program (Expr) @expr)
    "#});
}

// ============================================================================
// 10. OPTIONALS
// ============================================================================

#[test]
fn optional_first_child() {
    snap!(indoc! {r#"
        Test = (program (identifier)? @id (number) @n)
    "#});
}

#[test]
fn optional_null_injection() {
    snap!(indoc! {r#"
        Test = (function_declaration (decorator)? @dec)
    "#});
}

// ============================================================================
// 11. COMPREHENSIVE
// ============================================================================

#[test]
fn comprehensive_multi_definition() {
    snap!(indoc! {r#"
        Ident = (identifier) @name :: string
        Expression = [
            Literal: (number) @value
            Variable: (identifier) @name
        ]
        Assignment = (assignment_expression
            left: (identifier) @target
            right: (Expression) @value)
    "#});
}
