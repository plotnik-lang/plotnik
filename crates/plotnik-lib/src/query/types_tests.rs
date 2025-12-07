//! Type inference tests.

use crate::Query;
use indoc::indoc;

#[test]
fn capture_node_produces_node_field() {
    let query = Query::try_from("(identifier) @id").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { id: SyntaxNode };");
}

#[test]
fn multiple_captures_produce_multiple_fields() {
    let query = Query::try_from("(binary left: (_) @left right: (_) @right)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { left: SyntaxNode; right: SyntaxNode };");
}

#[test]
fn no_captures_produces_unit() {
    let query = Query::try_from("(identifier)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"");
}

#[test]
fn nested_capture_flattens() {
    let query = Query::try_from("(function name: (identifier) @name)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { name: SyntaxNode };");
}

#[test]
fn string_annotation() {
    let query = Query::try_from("(identifier) @name :: string").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { name: string };");
}

#[test]
fn named_type_annotation() {
    let query = Query::try_from("(identifier) @value :: MyType").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { value: MyType };");
}

#[test]
fn annotation_on_quantified_wraps_inner() {
    let query = Query::try_from("(identifier)+ @names :: string").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultNames = [string, ...string[]];

    type QueryResult = { names: [string, ...string[]] };
    ");
}

#[test]
fn capture_ref_produces_ref_type() {
    let input = indoc! {r#"
        Inner = (identifier) @name
        (wrapper (Inner) @inner)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Inner = { name: SyntaxNode };

    type QueryResult = { inner: Inner };
    ");
}

#[test]
fn ref_without_capture_contributes_nothing() {
    let input = indoc! {r#"
        Inner = (identifier) @name
        (wrapper (Inner))
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type Inner = { name: SyntaxNode };");
}

#[test]
fn optional_node() {
    let query = Query::try_from("(identifier)? @maybe").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultMaybe = SyntaxNode;

    type QueryResult = { maybe?: SyntaxNode };
    ");
}

#[test]
fn list_of_nodes() {
    let query = Query::try_from("(identifier)* @items").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultItems = SyntaxNode[];

    type QueryResult = { items: SyntaxNode[] };
    ");
}

#[test]
fn nonempty_list_of_nodes() {
    let query = Query::try_from("(identifier)+ @items").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultItems = [SyntaxNode, ...SyntaxNode[]];

    type QueryResult = { items: [SyntaxNode, ...SyntaxNode[]] };
    ");
}

#[test]
fn quantified_ref() {
    let input = indoc! {r#"
        Item = (item) @value
        (container (Item)+ @items)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Item = { value: SyntaxNode };

    type QueryResultItems = [Item, ...Item[]];

    type QueryResult = { items: [Item, ...Item[]] };
    ");
}

#[test]
fn quantifier_outside_capture() {
    let query = Query::try_from("((identifier) @id)*").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultId = SyntaxNode[];

    type QueryResult = { id: SyntaxNode[] };
    ");
}

#[test]
fn captured_seq_creates_nested_struct() {
    let query = Query::try_from("{(a) @x (b) @y} @pair").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultPair0 = { x: SyntaxNode; y: SyntaxNode };

    type QueryResult = { pair: QueryResultPair0 };
    ");
}

#[test]
fn captured_seq_in_tree() {
    let input = indoc! {r#"
        (function
            {(param) @p} @params
            (body) @body)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultParams0 = { p: SyntaxNode };

    type QueryResult = { params: QueryResultParams0; body: SyntaxNode };
    ");
}

#[test]
fn empty_captured_seq_is_node() {
    let query = Query::try_from("{} @empty").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { empty: SyntaxNode };");
}

#[test]
fn tagged_alt_produces_union() {
    let input = "[A: (a) @x B: (b) @y]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type QueryResultA = { x: SyntaxNode };

    type QueryResultB = { y: SyntaxNode };

    type QueryResult =
      | { tag: "A"; x: SyntaxNode }
      | { tag: "B"; y: SyntaxNode };
    "#);
}

#[test]
fn tagged_alt_as_definition() {
    let input = indoc! {r#"
        Expr = [
            Binary: (binary left: (_) @left right: (_) @right)
            Literal: (number) @value
        ]
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type ExprBinary = { left: SyntaxNode; right: SyntaxNode };

    type ExprLiteral = { value: SyntaxNode };

    type Expr =
      | { tag: "Binary"; left: SyntaxNode; right: SyntaxNode }
      | { tag: "Literal"; value: SyntaxNode };
    "#);
}

#[test]
fn tagged_branch_without_captures_is_unit() {
    let input = "[A: (a) B: (b)]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type QueryResult =
      | { tag: "A" }
      | { tag: "B" };
    "#);
}

#[test]
fn tagged_branch_with_ref() {
    let input = indoc! {r#"
        Rec = [Base: (a) Nested: (Rec)?] @value
        (Rec)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type RecValue =
      | { tag: "Base" }
      | { tag: "Nested"; value: Rec };

    type Rec = { value: RecValue };

    type RecValueNested = { value: Rec };
    "#);
}

#[test]
fn captured_tagged_alt() {
    let input = "(container [A: (a) B: (b)] @choice)";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type QueryResultChoice =
      | { tag: "A" }
      | { tag: "B" };

    type QueryResult = { choice: QueryResultChoice };
    "#);
}

#[test]
fn untagged_alt_same_capture_merges() {
    let input = "[(a) @x (b) @x]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { x: SyntaxNode };");
}

#[test]
fn untagged_alt_different_captures_becomes_optional() {
    let input = "[(a) @x (b) @y]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultXOpt = SyntaxNode;

    type QueryResultYOpt = SyntaxNode;

    type QueryResult = { x?: SyntaxNode; y?: SyntaxNode };
    ");
}

#[test]
fn untagged_alt_nested_alt_merges() {
    let input = "[(a) @x (b) @y [(c) @x (d) @y]]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultXOpt = SyntaxNode;

    type QueryResultYOpt = SyntaxNode;

    type QueryResult = { x?: SyntaxNode; y?: SyntaxNode };
    ");
}

#[test]
fn captured_untagged_alt_with_nested_fields() {
    let input = "[{(a) @x} {(b) @y}] @choice";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultChoiceXOpt = SyntaxNode;

    type QueryResultChoiceYOpt = SyntaxNode;

    type QueryResultChoice0 = { x?: SyntaxNode; y?: SyntaxNode };

    type QueryResult = { choice: QueryResultChoice0 };
    ");
}

#[test]
fn merge_same_type_unchanged() {
    let input = "[(identifier) @x (identifier) @x]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { x: SyntaxNode };");
}

#[test]
fn merge_absent_field_becomes_optional() {
    let input = "[(identifier) @x (number)]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultXOpt = SyntaxNode;

    type QueryResult = { x?: SyntaxNode };
    ");
}

#[test]
fn merge_list_and_nonempty_list_to_list() {
    let input = "[(a)* @x (b)+ @x]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultX = [SyntaxNode, ...SyntaxNode[]];

    type QueryResult = { x: [SyntaxNode, ...SyntaxNode[]] };
    ");
}

#[test]
fn merge_optional_and_required_to_optional() {
    let input = "[(a)? @x (b) @x]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultX = SyntaxNode;

    type QueryResult = { x?: SyntaxNode };
    ");
}

#[test]
fn self_recursive_type_marked_cyclic() {
    let input = "Expr = [(identifier) (call (Expr) @callee)]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Expr = { callee?: Expr };

    type ExprCalleeOpt = Expr;
    ");
}

#[test]
fn recursive_through_optional() {
    let input = indoc! {r#"
        Rec = (call function: (Rec)? @inner)
        (Rec)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Rec = { inner?: Rec };

    type RecInner = Rec;
    ");
}

#[test]
fn recursive_in_tagged_alt() {
    let input = indoc! {r#"
        Expr = [
            Ident: (identifier) @name
            Call: (call function: (Expr) @func)
        ]
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type ExprIdent = { name: SyntaxNode };

    type Expr =
      | { tag: "Ident"; name: SyntaxNode }
      | { tag: "Call"; func: Expr };

    type ExprCall = { func: Expr };
    "#);
}

#[test]
fn unnamed_last_def_is_default_query() {
    let input = "(program (identifier)* @items)";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultItems = SyntaxNode[];

    type QueryResult = { items: SyntaxNode[] };
    ");
}

#[test]
fn named_defs_plus_entry_point() {
    let input = indoc! {r#"
        Item = (item) @value
        (container (Item)* @items)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Item = { value: SyntaxNode };

    type QueryResultItems = Item[];

    type QueryResult = { items: Item[] };
    ");
}

#[test]
fn tagged_alt_at_entry_point() {
    let input = "[A: (a) @x B: (b) @y]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type QueryResultA = { x: SyntaxNode };

    type QueryResultB = { y: SyntaxNode };

    type QueryResult =
      | { tag: "A"; x: SyntaxNode }
      | { tag: "B"; y: SyntaxNode };
    "#);
}

#[test]
fn type_conflict_in_untagged_alt() {
    let input = "[(identifier) @x :: string (number) @x]";
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture `x` has conflicting types across branches
      |
    1 | [(identifier) @x :: string (number) @x]
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    ");
}

#[test]
fn incompatible_tagged_alts_in_merge() {
    let input = "[[A: (a) @x] @y [B: (b) @z] @y]";
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: tagged alternations with different variants cannot be merged
      |
    1 | [[A: (a) @x] @y [B: (b) @z] @y]
      |  ^^^^^^^^^^^    ----------- incompatible
    ");
}

#[test]
fn duplicate_capture_in_sequence() {
    let input = "{(a) @x (b) @x}";
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture `@x` already used in this scope
      |
    1 | {(a) @x (b) @x}
      |       -      ^
      |       |
      |       first use
    ");
}

#[test]
fn duplicate_capture_nested() {
    let input = "(foo (a) @x (bar (b) @x))";
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture `@x` already used in this scope
      |
    1 | (foo (a) @x (bar (b) @x))
      |           - first use ^
    ");
}

#[test]
fn wildcard_capture() {
    let query = Query::try_from("(_) @node").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { node: SyntaxNode };");
}

#[test]
fn anonymous_node_capture() {
    let query = Query::try_from("_ @anon").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { anon: SyntaxNode };");
}

#[test]
fn string_literal_capture() {
    let query = Query::try_from(r#""if" @kw"#).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { kw: SyntaxNode };");
}

#[test]
fn field_value_capture() {
    let query = Query::try_from("(call name: (identifier) @name)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @"type QueryResult = { name: SyntaxNode };");
}

#[test]
fn deeply_nested_seq() {
    let query = Query::try_from("{{{(identifier) @x}}} @outer").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type QueryResultOuter0 = { x: SyntaxNode };

    type QueryResult = { outer: QueryResultOuter0 };
    ");
}

#[test]
fn same_tag_in_branches_merges() {
    let input = "[[A: (a)] @x [A: (b)] @x]";
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r#"
    type QueryResultX =
      | { tag: "A" };

    type QueryResult = { x: QueryResultX };
    "#);
}

#[test]
fn annotation_on_captured_ref() {
    let input = indoc! {r#"
        Inner = (identifier) @name
        (wrapper (Inner) @inner :: CustomType)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type Inner = { name: SyntaxNode };

    type QueryResult = { inner: Inner };
    ");
}

#[test]
fn multiple_defs_with_refs() {
    let input = indoc! {r#"
        A = (a) @x
        B = (b (A) @a)
        C = (c (B) @b)
        (root (C) @c)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_types(), @r"
    type A = { x: SyntaxNode };

    type B = { a: A };

    type C = { b: B };

    type QueryResult = { c: C };
    ");
}
