use crate::Query;
use indoc::indoc;

#[test]
fn tree_is_one() {
    let query = Query::try_from("(identifier)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ identifier
    ");
}

#[test]
fn singleton_seq_is_one() {
    let query = Query::try_from("{(identifier)}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Seq¹
          NamedNode¹ identifier
    ");
}

#[test]
fn nested_singleton_seq_is_one() {
    let query = Query::try_from("{{{(identifier)}}}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Seq¹
          Seq¹
            Seq¹
              NamedNode¹ identifier
    ");
}

#[test]
fn multi_seq_is_many() {
    let query = Query::try_from("{(a) (b)}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def⁺
        Seq⁺
          NamedNode¹ a
          NamedNode¹ b
    ");
}

#[test]
fn alt_is_one() {
    let query = Query::try_from("[(a) (b)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch¹
            NamedNode¹ a
          Branch¹
            NamedNode¹ b
    ");
}

#[test]
fn alt_with_seq_branches() {
    let input = indoc! {r#"
    [{(a) (b)} (c)]
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch⁺
            Seq⁺
              NamedNode¹ a
              NamedNode¹ b
          Branch¹
            NamedNode¹ c
    ");
}

#[test]
fn ref_to_tree_is_one() {
    let input = indoc! {r#"
    X = (identifier)
    (call (X))
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root⁺
      Def¹ X
        NamedNode¹ identifier
      Def¹
        NamedNode¹ call
          Ref¹ X
    ");
}

#[test]
fn ref_to_seq_is_many() {
    let input = indoc! {r#"
    X = {(a) (b)}
    (call (X))
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root⁺
      Def⁺ X
        Seq⁺
          NamedNode¹ a
          NamedNode¹ b
      Def¹
        NamedNode¹ call
          Ref⁺ X
    ");
}

#[test]
fn field_with_tree() {
    let query = Query::try_from("(call name: (identifier))").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ call
          FieldExpr¹ name:
            NamedNode¹ identifier
    ");
}

#[test]
fn field_with_alt() {
    let query = Query::try_from("(call name: [(identifier) (string)])").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ call
          FieldExpr¹ name:
            Alt¹
              Branch¹
                NamedNode¹ identifier
              Branch¹
                NamedNode¹ string
    ");
}

#[test]
fn field_with_seq_error() {
    let query = Query::try_from("(call name: {(a) (b)})").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ call
          FieldExpr¹ name:
            Seq⁺
              NamedNode¹ a
              NamedNode¹ b
    ");
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `name` must match exactly one node, not a sequence
      |
    1 | (call name: {(a) (b)})
      |             ^^^^^^^^^
    ");
}

#[test]
fn field_with_ref_to_seq_error() {
    let input = indoc! {r#"
    X = {(a) (b)}
    (call name: (X))
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root⁺
      Def⁺ X
        Seq⁺
          NamedNode¹ a
          NamedNode¹ b
      Def¹
        NamedNode¹ call
          FieldExpr¹ name:
            Ref⁺ X
    ");
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field `name` must match exactly one node, not a sequence
      |
    2 | (call name: (X))
      |             ^^^
    ");
}

#[test]
fn quantifier_preserves_inner_arity() {
    let query = Query::try_from("(identifier)*").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        QuantifiedExpr¹ *
          NamedNode¹ identifier
    ");
}

#[test]
fn capture_preserves_inner_arity() {
    let query = Query::try_from("(identifier) @name").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        CapturedExpr¹ @name
          NamedNode¹ identifier
    ");
}

#[test]
fn capture_on_seq() {
    let query = Query::try_from("{(a) (b)} @items").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def⁺
        CapturedExpr⁺ @items
          Seq⁺
            NamedNode¹ a
            NamedNode¹ b
    ");
}

#[test]
fn complex_nested_arities() {
    let input = indoc! {r#"
    Stmt = [(expr_stmt) (return_stmt)]
    (function_definition
        name: (identifier) @name
        body: (block (Stmt)* @stmts))
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root⁺
      Def¹ Stmt
        Alt¹
          Branch¹
            NamedNode¹ expr_stmt
          Branch¹
            NamedNode¹ return_stmt
      Def¹
        NamedNode¹ function_definition
          CapturedExpr¹ @name
            FieldExpr¹ name:
              NamedNode¹ identifier
          FieldExpr¹ body:
            NamedNode¹ block
              CapturedExpr¹ @stmts
                QuantifiedExpr¹ *
                  Ref¹ Stmt
    ");
}

#[test]
fn tagged_alt_arities() {
    let input = indoc! {r#"
    [Ident: (identifier) Num: (number)]
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Alt¹
          Branch¹ Ident:
            NamedNode¹ identifier
          Branch¹ Num:
            NamedNode¹ number
    ");
}

#[test]
fn anchor_has_no_arity() {
    let query = Query::try_from("(block . (statement))").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ block
          .
          NamedNode¹ statement
    ");
}

#[test]
fn negated_field_has_no_arity() {
    let query = Query::try_from("(function !async)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ function
          NegatedField !async
    ");
}

#[test]
fn tree_with_wildcard_type() {
    let query = Query::try_from("(_)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ (any)
    ");
}

#[test]
fn bare_wildcard_is_one() {
    let query = Query::try_from("_").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        AnonymousNode¹ (any)
    ");
}

#[test]
fn empty_seq_is_one() {
    let query = Query::try_from("{}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Seq¹
    ");
}

#[test]
fn literal_is_one() {
    let query = Query::try_from(r#""if""#).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r#"
    Root¹
      Def¹
        AnonymousNode¹ "if"
    "#);
}

#[test]
fn invalid_error_node() {
    let query = Query::try_from("(foo %)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_cst_with_arities(), @r#"
    Root¹
      Def¹
        Tree¹
          ParenOpen "("
          Id "foo"
          Error⁻
            Garbage "%"
          ParenClose ")"
    "#);
}

#[test]
fn invalid_undefined_ref() {
    let query = Query::try_from("(Undefined)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def⁻
        Ref⁻ Undefined
    ");
}

#[test]
fn invalid_branch_without_body() {
    let query = Query::try_from("[A:]").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        Alt¹
          Branchˣ A:
    ");
}

#[test]
fn invalid_ref_to_bodyless_def() {
    let input = indoc! {r#"
    X = %
    (X)
    "#};
    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root⁺
      Defˣ X
      Defˣ
      Def⁻
        Ref⁻ X
    ");
}

#[test]
fn invalid_capture_without_inner() {
    // Error recovery: `extra` is invalid, but `@y` still creates a Capture node
    let query = Query::try_from("(call extra @y)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ call
          CapturedExpr⁻ @y
    ");
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (call extra @y)
      |       ^^^^^
    ");
}

#[test]
fn invalid_capture_without_inner_standalone() {
    // Standalone capture without preceding expression
    let query = Query::try_from("@x").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `@` must follow an expression to capture
      |
    1 | @x
      | ^
    ");
}

#[test]
fn invalid_multiple_captures_with_error() {
    let query = Query::try_from("(call (Undefined) @x extra @y)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ call
          CapturedExpr⁻ @x
            Ref⁻ Undefined
          CapturedExpr⁻ @y
    ");
}

#[test]
fn invalid_quantifier_without_inner() {
    // Error recovery: `extra` is invalid, but `*` still creates a Quantifier node
    let query = Query::try_from("(foo extra*)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_with_arities(), @r"
    Root¹
      Def¹
        NamedNode¹ foo
          QuantifiedExpr⁻ *
    ");
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (foo extra*)
      |      ^^^^^
    ");
}
