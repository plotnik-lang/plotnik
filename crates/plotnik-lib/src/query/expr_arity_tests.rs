use crate::Query;
use indoc::indoc;

#[test]
fn tree_is_one() {
    let input = "Q = (identifier)";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ identifier
    ");
}

#[test]
fn singleton_seq_is_one() {
    let input = "Q = {(identifier)}";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        Seq¹
          NamedNode¹ identifier
    ");
}

#[test]
fn nested_singleton_seq_is_one() {
    let input = "Q = {{{(identifier)}}}";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        Seq¹
          Seq¹
            Seq¹
              NamedNode¹ identifier
    ");
}

#[test]
fn multi_seq_is_many() {
    let input = "Q = {(a) (b)}";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def⁺ Q
        Seq⁺
          NamedNode¹ a
          NamedNode¹ b
    ");
}

#[test]
fn alt_is_one() {
    let input = "Q = [(a) (b)]";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
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
    Q = [{(a) (b)} (c)]
    "#};

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
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
    Q = (call (X))
    "#};

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root⁺
      Def¹ X
        NamedNode¹ identifier
      Def¹ Q
        NamedNode¹ call
          Ref¹ X
    ");
}

#[test]
fn ref_to_seq_is_many() {
    let input = indoc! {r#"
    X = {(a) (b)}
    Q = (call (X))
    "#};

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root⁺
      Def⁺ X
        Seq⁺
          NamedNode¹ a
          NamedNode¹ b
      Def¹ Q
        NamedNode¹ call
          Ref⁺ X
    ");
}

#[test]
fn field_with_tree() {
    let input = "Q = (call name: (identifier))";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ call
          FieldExpr¹ name:
            NamedNode¹ identifier
    ");
}

#[test]
fn field_with_alt() {
    let input = "Q = (call name: [(identifier) (string)])";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
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
    let input = "Q = (call name: {(a) (b)})";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field `name` must match exactly one node, not a sequence
      |
    1 | Q = (call name: {(a) (b)})
      |                 ^^^^^^^^^
    ");
}

#[test]
fn field_with_ref_to_seq_error() {
    let input = indoc! {r#"
    X = {(a) (b)}
    Q = (call name: (X))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field `name` must match exactly one node, not a sequence
      |
    1 | X = {(a) (b)}
      |     --------- defined here
    2 | Q = (call name: (X))
      |                 ^^^
    ");
}

#[test]
fn quantifier_preserves_inner_arity() {
    let input = "Q = (identifier)*";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        QuantifiedExpr¹ *
          NamedNode¹ identifier
    ");
}

#[test]
fn capture_preserves_inner_arity() {
    let input = "Q = (identifier) @name";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        CapturedExpr¹ @name
          NamedNode¹ identifier
    ");
}

#[test]
fn capture_on_seq() {
    let input = "Q = {(a) (b)} @items";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def⁺ Q
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
    Q = (function_definition
        name: (identifier) @name
         body: (block (Stmt)* @stmts))
    "#};

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root⁺
      Def¹ Stmt
        Alt¹
          Branch¹
            NamedNode¹ expr_stmt
          Branch¹
            NamedNode¹ return_stmt
      Def¹ Q
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
    Q = [Ident: (identifier) Num: (number)]
    "#};

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        Alt¹
          Branch¹ Ident:
            NamedNode¹ identifier
          Branch¹ Num:
            NamedNode¹ number
    ");
}

#[test]
fn anchor_has_no_arity() {
    let input = "Q = (block . (statement))";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ block
          .
          NamedNode¹ statement
    ");
}

#[test]
fn negated_field_has_no_arity() {
    let input = "Q = (function !async)";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ function
          NegatedField !async
    ");
}

#[test]
fn tree_with_wildcard_type() {
    let input = "Q = (_)";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        NamedNode¹ (any)
    ");
}

#[test]
fn bare_wildcard_is_one() {
    let input = "Q = _";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        AnonymousNode¹ (any)
    ");
}

#[test]
fn empty_seq_is_one() {
    let input = "Q = {}";

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r"
    Root¹
      Def¹ Q
        Seq¹
    ");
}

#[test]
fn literal_is_one() {
    let input = r#"
        Q = "if"
    "#;

    let res = Query::expect_valid_arities(input);

    insta::assert_snapshot!(res, @r#"
    Root¹
      Def¹ Q
        AnonymousNode¹ "if"
    "#);
}

#[test]
fn invalid_error_node() {
    let input = "Q = (foo %)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: unexpected token; not valid inside a node — try `(child)` or close with `)`
      |
    1 | Q = (foo %)
      |          ^
    ");
}

#[test]
fn invalid_undefined_ref() {
    let input = "Q = (Undefined)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Undefined` is not defined
      |
    1 | Q = (Undefined)
      |      ^^^^^^^^^
    ");
}

#[test]
fn invalid_branch_without_body() {
    let input = "Q = [A:]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression; after `Label:`
      |
    1 | Q = [A:]
      |        ^
    ");
}

#[test]
fn invalid_ref_to_bodyless_def() {
    let input = indoc! {r#"
    X = %
    Q = (X)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression; after `=` in definition
      |
    1 | X = %
      |     ^

    error: `X` is not defined
      |
    2 | Q = (X)
      |      ^
    ");
}

#[test]
fn invalid_capture_without_inner() {
    // Error recovery: `extra` is invalid, but `@y` still creates a Capture node
    let input = "Q = (call extra @y)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Q = (call extra @y)
      |           ^^^^^
    ");
}

#[test]
fn invalid_capture_without_inner_standalone() {
    // Standalone capture without preceding expression
    let input = "Q = @x";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: expected an expression; after `=` in definition
      |
    1 | Q = @x
      |     ^
    ");
}

#[test]
fn invalid_multiple_captures_with_error() {
    let input = "Q = (call (Undefined) @x extra @y)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Undefined` is not defined
      |
    1 | Q = (call (Undefined) @x extra @y)
      |            ^^^^^^^^^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Q = (call (Undefined) @x extra @y)
      |                          ^^^^^
    ");
}

#[test]
fn invalid_quantifier_without_inner() {
    // Error recovery: `extra` is invalid, but `*` still creates a Quantifier node
    let input = "Q = (foo extra*)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Q = (foo extra*)
      |          ^^^^^
    ");
}
