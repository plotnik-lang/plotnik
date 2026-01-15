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

// Nodes

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

// Captures

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
        Test = (array (array (identifier) @c) @b) @a
    "#});
}

#[test]
fn captures_deeply_nested() {
    snap!(indoc! {r#"
        Test = (array (array (array (identifier) @d) @c) @b) @a
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
        Test = {(identifier) @a (number) @b} @item
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

#[test]
fn captures_struct_with_type_annotation() {
    // Type annotation on struct capture should name the struct, not create an alias
    snap!(indoc! {r#"
        Test = {(identifier) @fn} @outer :: FunctionInfo
    "#});
}

#[test]
fn captures_enum_with_type_annotation() {
    // Type annotation on tagged alternation should name the enum
    snap!(indoc! {r#"
        Test = [A: (identifier) @id B: (number) @num] @expr :: Expression
    "#});
}

// Fields

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
        Test = (pair key: (property_identifier) @key -value)
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

// Quantifiers

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
        Test = (array (Collect))
    "#});
}

// Sequences

#[test]
fn sequences_basic() {
    snap!(indoc! {r#"
        Test = (array {(identifier) (number)})
    "#});
}

#[test]
fn sequences_with_captures() {
    snap!(indoc! {r#"
        Test = (array {(identifier) @a (number) @b})
    "#});
}

#[test]
fn sequences_nested() {
    snap!(indoc! {r#"
        Test = (array {(identifier) {(number) (string)} (null)})
    "#});
}

#[test]
fn sequences_in_quantifier() {
    // Sequence with internal captures - valid for struct array
    snap!(indoc! {r#"
        Test = (array {(identifier) @id (number) @num}* @items)
    "#});
}

// Alternations

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
fn alternations_tagged_with_definition_ref() {
    snap!(indoc! {r#"
        Inner = (identifier) @name
        Test = [A: (Inner)  B: (number) @b] @item
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

#[test]
fn alternations_tagged_in_field_constraint() {
    // Regression test: captured tagged alternation as field value should not emit Node effect.
    // The capture `@kind` applies to the field expression, but the value determines
    // whether it's a structured scope (enum in this case).
    snap!(indoc! {r#"
        Test = (pair key: [A: (identifier) @a B: (number)] @kind)
    "#});
}

// Anchors

#[test]
fn anchors_between_siblings() {
    snap!(indoc! {r#"
        Test = (array (identifier) . (number))
    "#});
}

#[test]
fn anchors_first_child() {
    snap!(indoc! {r#"
        Test = (array . (identifier))
    "#});
}

#[test]
fn anchors_last_child() {
    snap!(indoc! {r#"
        Test = (array (identifier) .)
    "#});
}

#[test]
fn anchors_with_anonymous() {
    snap!(indoc! {r#"
        Test = (binary_expression "+" . (identifier))
    "#});
}

#[test]
fn anchors_no_anchor() {
    snap!(indoc! {r#"
        Test = (array (identifier) (number))
    "#});
}

// Named expressions

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
        Inner = (call_expression (identifier) @name)
        Outer = (array {(Inner) @item}* @items)
    "#});
}

// Recursion

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

// Optionals

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

// Comprehensive

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
