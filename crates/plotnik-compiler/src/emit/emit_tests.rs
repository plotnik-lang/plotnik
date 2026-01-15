//! Comprehensive bytecode emission tests.
//!
//! Tests are organized by language feature and use file-based snapshots.
//! Each test verifies the bytecode output for a specific language construct.

use crate::shot_bytecode;

// Nodes

#[test]
fn nodes_named() {
    shot_bytecode!(r#"
        Test = (identifier) @id
    "#);
}

#[test]
fn nodes_anonymous() {
    shot_bytecode!(r#"
        Test = (binary_expression "+" @op)
    "#);
}

#[test]
fn nodes_wildcard_any() {
    shot_bytecode!(r#"
        Test = (pair key: _ @key)
    "#);
}

#[test]
fn nodes_wildcard_named() {
    shot_bytecode!(r#"
        Test = (pair key: (_) @key)
    "#);
}

#[test]
fn nodes_error() {
    shot_bytecode!(r#"
        Test = (ERROR) @err
    "#);
}

#[test]
fn nodes_missing() {
    shot_bytecode!(r#"
        Test = (MISSING) @m
    "#);
}

// Captures

#[test]
fn captures_basic() {
    shot_bytecode!(r#"
        Test = (identifier) @name
    "#);
}

#[test]
fn captures_multiple() {
    shot_bytecode!(r#"
        Test = (binary_expression (identifier) @a (number) @b)
    "#);
}

#[test]
fn captures_nested_flat() {
    shot_bytecode!(r#"
        Test = (array (array (identifier) @c) @b) @a
    "#);
}

#[test]
fn captures_deeply_nested() {
    shot_bytecode!(r#"
        Test = (array (array (array (identifier) @d) @c) @b) @a
    "#);
}

#[test]
fn captures_with_type_string() {
    shot_bytecode!(r#"
        Test = (identifier) @name :: string
    "#);
}

#[test]
fn captures_with_type_custom() {
    shot_bytecode!(r#"
        Test = (identifier) @name :: Identifier
    "#);
}

#[test]
fn captures_struct_scope() {
    shot_bytecode!(r#"
        Test = {(identifier) @a (number) @b} @item
    "#);
}

#[test]
fn captures_wrapper_struct() {
    shot_bytecode!(r#"
        Test = {{(identifier) @id (number) @num} @row}* @rows
    "#);
}

#[test]
fn captures_optional_wrapper_struct() {
    shot_bytecode!(r#"
        Test = {{(identifier) @id} @inner}? @outer
    "#);
}

#[test]
fn captures_struct_with_type_annotation() {
    shot_bytecode!(r#"
        Test = {(identifier) @fn} @outer :: FunctionInfo
    "#);
}

#[test]
fn captures_enum_with_type_annotation() {
    shot_bytecode!(r#"
        Test = [A: (identifier) @id B: (number) @num] @expr :: Expression
    "#);
}

// Fields

#[test]
fn fields_single() {
    shot_bytecode!(r#"
        Test = (function_declaration name: (identifier) @name)
    "#);
}

#[test]
fn fields_multiple() {
    shot_bytecode!(r#"
        Test = (binary_expression
            left: (_) @left
            right: (_) @right)
    "#);
}

#[test]
fn fields_negated() {
    shot_bytecode!(r#"
        Test = (pair key: (property_identifier) @key -value)
    "#);
}

#[test]
fn fields_alternation() {
    shot_bytecode!(r#"
        Test = (call_expression function: [(identifier) @fn (number) @num])
    "#);
}

// Quantifiers

#[test]
fn quantifiers_optional() {
    shot_bytecode!(r#"
        Test = (function_declaration (decorator)? @dec)
    "#);
}

#[test]
fn quantifiers_star() {
    shot_bytecode!(r#"
        Test = (identifier)* @items
    "#);
}

#[test]
fn quantifiers_plus() {
    shot_bytecode!(r#"
        Test = (identifier)+ @items
    "#);
}

#[test]
fn quantifiers_optional_nongreedy() {
    shot_bytecode!(r#"
        Test = (function_declaration (decorator)?? @dec)
    "#);
}

#[test]
fn quantifiers_star_nongreedy() {
    shot_bytecode!(r#"
        Test = (identifier)*? @items
    "#);
}

#[test]
fn quantifiers_plus_nongreedy() {
    shot_bytecode!(r#"
        Test = (identifier)+? @items
    "#);
}

#[test]
fn quantifiers_struct_array() {
    shot_bytecode!(r#"
        Test = (array {(identifier) @a (number) @b}* @items)
    "#);
}

#[test]
fn quantifiers_first_child_array() {
    shot_bytecode!(r#"
        Test = (array (identifier)* @ids (number) @n)
    "#);
}

#[test]
fn quantifiers_repeat_navigation() {
    shot_bytecode!(r#"
        Test = (function_declaration (decorator)* @decs)
    "#);
}

#[test]
fn quantifiers_sequence_in_called_def() {
    shot_bytecode!(r#"
        Item = (identifier) @name
        Collect = {(Item) @item}* @items
        Test = (array (Collect))
    "#);
}

// Sequences

#[test]
fn sequences_basic() {
    shot_bytecode!(r#"
        Test = (array {(identifier) (number)})
    "#);
}

#[test]
fn sequences_with_captures() {
    shot_bytecode!(r#"
        Test = (array {(identifier) @a (number) @b})
    "#);
}

#[test]
fn sequences_nested() {
    shot_bytecode!(r#"
        Test = (array {(identifier) {(number) (string)} (null)})
    "#);
}

#[test]
fn sequences_in_quantifier() {
    shot_bytecode!(r#"
        Test = (array {(identifier) @id (number) @num}* @items)
    "#);
}

// Alternations

#[test]
fn alternations_unlabeled() {
    shot_bytecode!(r#"
        Test = [(identifier) @id (string) @str]
    "#);
}

#[test]
fn alternations_labeled() {
    shot_bytecode!(r#"
        Test = [
            A: (identifier) @a
            B: (number) @b
        ]
    "#);
}

#[test]
fn alternations_null_injection() {
    shot_bytecode!(r#"
        Test = [(identifier) @x (number) @y]
    "#);
}

#[test]
fn alternations_captured() {
    shot_bytecode!(r#"
        Test = [(identifier) (number)] @value
    "#);
}

#[test]
fn alternations_captured_tagged() {
    shot_bytecode!(r#"
        Test = [A: (identifier) @a  B: (number) @b] @item
    "#);
}

#[test]
fn alternations_tagged_with_definition_ref() {
    shot_bytecode!(r#"
        Inner = (identifier) @name
        Test = [A: (Inner)  B: (number) @b] @item
    "#);
}

#[test]
fn alternations_in_quantifier() {
    shot_bytecode!(r#"
        Test = (object { [A: (pair) @a  B: (shorthand_property_identifier) @b] @item }* @items)
    "#);
}

#[test]
fn alternations_no_internal_captures() {
    shot_bytecode!(r#"
        Test = (program [(identifier) (number)] @x)
    "#);
}

#[test]
fn alternations_tagged_in_field_constraint() {
    shot_bytecode!(r#"
        Test = (pair key: [A: (identifier) @a B: (number)] @kind)
    "#);
}

// Anchors

#[test]
fn anchors_between_siblings() {
    shot_bytecode!(r#"
        Test = (array (identifier) . (number))
    "#);
}

#[test]
fn anchors_first_child() {
    shot_bytecode!(r#"
        Test = (array . (identifier))
    "#);
}

#[test]
fn anchors_last_child() {
    shot_bytecode!(r#"
        Test = (array (identifier) .)
    "#);
}

#[test]
fn anchors_with_anonymous() {
    shot_bytecode!(r#"
        Test = (binary_expression "+" . (identifier))
    "#);
}

#[test]
fn anchors_no_anchor() {
    shot_bytecode!(r#"
        Test = (array (identifier) (number))
    "#);
}

// Named expressions

#[test]
fn definitions_single() {
    shot_bytecode!(r#"
        Foo = (identifier) @id
    "#);
}

#[test]
fn definitions_multiple() {
    shot_bytecode!(r#"
        Foo = (identifier) @id
        Bar = (string) @str
    "#);
}

#[test]
fn definitions_reference() {
    shot_bytecode!(r#"
        Expression = [(identifier) @name (number) @value]
        Root = (function_declaration name: (identifier) @name)
    "#);
}

#[test]
fn definitions_nested_capture() {
    shot_bytecode!(r#"
        Inner = (call_expression (identifier) @name)
        Outer = (array {(Inner) @item}* @items)
    "#);
}

// Recursion

#[test]
fn recursion_simple() {
    shot_bytecode!(r#"
        Expr = [
            Lit: (number) @value :: string
            Rec: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]
    "#);
}

#[test]
fn recursion_with_structured_result() {
    shot_bytecode!(r#"
        Expr = [
          Lit: (number) @value :: string
          Nested: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]

        Test = (program (Expr) @expr)
    "#);
}

// Optionals

#[test]
fn optional_first_child() {
    shot_bytecode!(r#"
        Test = (program (identifier)? @id (number) @n)
    "#);
}

#[test]
fn optional_null_injection() {
    shot_bytecode!(r#"
        Test = (function_declaration (decorator)? @dec)
    "#);
}

// Optimization: prefix collapse

#[test]
fn opt_prefix_collapse() {
    shot_bytecode!(r#"
        Test = [(object (pair)) (object (string))]
    "#);
}

// Comprehensive

#[test]
fn comprehensive_multi_definition() {
    shot_bytecode!(r#"
        Ident = (identifier) @name :: string
        Expression = [
            Literal: (number) @value
            Variable: (identifier) @name
        ]
        Assignment = (assignment_expression
            left: (identifier) @target
            right: (Expression) @value)
    "#);
}
