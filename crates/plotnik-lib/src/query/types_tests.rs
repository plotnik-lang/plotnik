//! Type inference tests.

use crate::Query;
use indoc::indoc;

#[test]
fn comprehensive_type_inference() {
    let input = indoc! {r#"
        // Simple capture → flat struct with Node field
        Simple = (identifier) @id

        // Multiple captures → flat struct
        BinaryOp = (binary_expression
            left: (_) @left
            operator: _ @op
            right: (_) @right)

        // :: string annotation → String type
        WithString = (identifier) @name :: string

        // :: TypeName annotation → named type
        Named = (identifier) @value :: MyType

        // Ref usage → type reference
        UsingRef = (statement (BinaryOp) @expr)

        // Nested seq with capture → synthetic key
        Nested = (function
            {(param) @p} @params
            (body) @body)

        // Quantifiers on captures
        WithQuantifiers = (class
            (decorator)? @maybe_dec
            (method)* @methods
            (field)+ @fields)

        // Tagged alternation → TaggedUnion
        TaggedAlt = [
            Assign: (assignment left: (_) @target)
            Call: (call function: (_) @func)
        ]

        // Untagged alternation → merged struct
        UntaggedAlt = [
            (assignment left: (_) @left right: (_) @right)
            (call function: (_) @left)
        ]

        // Entry point (unnamed last def) → DefaultQuery
        (program (Simple)* @items)
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    Simple = { #Node @id }
    BinaryOp = { #Node @left #Node @op #Node @right }
    WithString = { #string @name }
    Named = { MyType @value }
    UsingRef = { BinaryOp @expr }
    <Nested params> = { #Node @p }
    Nested = { <Nested params> @params #Node @body }
    <Node Opt> = #Node?
    <Node List> = #Node+
    WithQuantifiers = { <Node Opt> @maybe_dec <Node List> @methods <Node List> @fields }
    <TaggedAlt Assign> = { #Node @target }
    <TaggedAlt Call> = { #Node @func }
    TaggedAlt = [ Assign: <TaggedAlt Assign> Call: <TaggedAlt Call> ]
    <right opt> = #Node?
    UntaggedAlt = { #Node @left <right opt> @right }
    <Simple List> = Simple*
    #DefaultQuery = { <Simple List> @items }
    ");
}

#[test]
fn type_conflict_in_untagged_alt() {
    let input = indoc! {r#"
        Conflict = [
            (identifier) @x :: string
            (number) @x
        ] @result
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture `x` has conflicting types across branches
      |
    1 |   Conflict = [
      |  ____________^
    2 | |     (identifier) @x :: string
    3 | |     (number) @x
    4 | | ] @result
      | |_^
    ");
}

#[test]
fn nested_tagged_alt_with_annotation() {
    let input = indoc! {r#"
        Expr = [
            Binary: (binary_expression
                left: (Expr) @left
                right: (Expr) @right)
            Literal: (number) @value :: string
        ]
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <Expr Binary> = { #Node @left #Node @right }
    <Expr Literal> = { #string @value }
    Expr = [ Binary: <Expr Binary> Literal: <Expr Literal> ]
    ");
}

#[test]
fn captured_ref_becomes_type_reference() {
    let input = indoc! {r#"
        Inner = (identifier) @name :: string
        Outer = (wrapper (Inner) @inner)
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    Inner = { #string @name }
    Outer = { Inner @inner }
    ");
}

#[test]
fn empty_captures_produce_unit() {
    let input = "(empty_node)";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"#DefaultQuery = ()");
}

#[test]
fn quantified_ref() {
    let input = indoc! {r#"
        Item = (item) @value
        List = (container (Item)+ @items)
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    Item = { #Node @value }
    <Item List> = Item+
    List = { <Item List> @items }
    ");
}
