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
    <Nested params 0> = { #Node @p }
    Nested = { <Nested params 0> @params #Node @body }
    <opt node> = #Node*
    <nonempty node> = #Node+
    WithQuantifiers = { <opt node> @maybe_dec <opt node> @methods <nonempty node> @fields }
    <TaggedAlt Assign> = { #Node @target }
    <TaggedAlt Call> = { #Node @func }
    TaggedAlt = [ Assign: <TaggedAlt Assign> Call: <TaggedAlt Call> ]
    <right opt> = #Node?
    UntaggedAlt = { #Node @left <right opt> @right }
    <SimpleWrapped> = Simple*
    #DefaultQuery = { <SimpleWrapped> @items }
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
    <Expr Binary> = { Expr @left Expr @right }
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
    <ItemWrapped> = Item+
    List = { <ItemWrapped> @items }
    ");
}

#[test]
fn recursive_type_with_annotation_preserves_fields() {
    let input = r#"Func = (function_declaration name: (identifier) @name) @func :: Func (Func)"#;

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    Func = { #Node @name Func @func }
    #DefaultQuery = ()
    ");
}

#[test]
fn anonymous_tagged_alt_uses_default_query_name() {
    let input = "[A: (identifier) @id B: (number) @num]";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <A> = { #Node @id }
    <B> = { #Node @num }
    #DefaultQuery = [ A: <A> B: <B> ]
    ");
}

#[test]
fn tagged_union_branch_with_ref() {
    let input = "Rec = [Base: (a) Rec: (Rec)?] (Rec)";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <Rec Base> = ()
    <RecWrapped> = Rec?
    <Rec Rec> = { <RecWrapped> @value }
    Rec = [ Base: <Rec Base> Rec: <Rec Rec> ]
    #DefaultQuery = ()
    ");
}

#[test]
fn nested_tagged_alts_in_untagged_alt_conflict() {
    // Each branch captures @x with different TaggedUnion types
    // Branch 1: @x is TaggedUnion with variant A
    // Branch 2: @x is TaggedUnion with variant B
    // This is a type conflict - different structures under same capture name
    let input = "[[A: (a) @aa] @x [B: (b) @bb] @x]";

    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <x A> = { #Node @aa }
    <x> = [ A: <x A> ]
    <x B> = { #Node @bb }
    #DefaultQuery = { <x> @x }
    ");
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: tagged alternations with different variants cannot be merged
      |
    1 | [[A: (a) @aa] @x [B: (b) @bb] @x]
      |  ^^^^^^^^^^^^    ------------ incompatible
    ");
}

#[test]
fn nested_untagged_alts_merge_fields() {
    // Each branch captures @x with different struct types
    // Branch 1: @x has field @y
    // Branch 2: @x has field @z
    // These get merged: fields from both branches become optional
    let input = "[[(a) @y] @x [(b) @z] @x]";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <x 0> = { #Node @y }
    <x 1> = { #Node @z }
    <x y opt> = #Node?
    <x z opt> = #Node?
    <x merged> = { <x y opt> @y <x z opt> @z }
    #DefaultQuery = { <x merged> @x }
    ");
}

#[test]
fn list_vs_nonempty_list_merged_to_list() {
    // Different quantifiers: * (List) vs + (NonEmptyList)
    // These merge to List (the more general type)
    let input = "[(a)* @x (b)+ @x]";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <opt node> = #Node*
    <nonempty node> = #Node+
    <list merged> = #Node*
    #DefaultQuery = { <list merged> @x }
    ");
}

#[test]
fn same_variant_name_across_branches_merges() {
    // Both branches have variant A - should merge correctly
    let input = "[[A: (a)] @x [A: (b)] @x]";

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <x A> = ()
    <x> = [ A: <x A> ]
    #DefaultQuery = { <x> @x }
    ");
}

#[test]
fn recursive_ref_through_optional_field() {
    let input = indoc! {r#"
        Rec = (call_expression function: (Rec)? @inner)
        (Rec)
    "#};

    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    <RecWrapped> = Rec?
    Rec = { <RecWrapped> @inner }
    #DefaultQuery = ()
    ");
}
