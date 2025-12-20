use crate::Query;
use indoc::indoc;

#[test]
fn simple_tree() {
    let res = Query::expect_valid_ast("Q = (identifier)");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode identifier
    ");
}

#[test]
fn nested_tree() {
    let input = indoc! {r#"
    Q = (function_definition name: (identifier))
    "#};

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode function_definition
          FieldExpr name:
            NamedNode identifier
    ");
}

#[test]
fn wildcard() {
    let res = Query::expect_valid_ast("Q = (_)");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode (any)
    ");
}

#[test]
fn literal() {
    let res = Query::expect_valid_ast(r#"Q = "if""#);
    insta::assert_snapshot!(res, @r#"
    Root
      Def Q
        AnonymousNode "if"
    "#);
}

#[test]
fn capture() {
    let res = Query::expect_valid_ast("Q = (identifier) @name");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        CapturedExpr @name
          NamedNode identifier
    ");
}

#[test]
fn capture_with_type() {
    let res = Query::expect_valid_ast("Q = (identifier) @name :: string");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        CapturedExpr @name :: string
          NamedNode identifier
    ");
}

#[test]
fn named_definition() {
    let res = Query::expect_valid_ast("Expr = (expression)");
    insta::assert_snapshot!(res, @r"
    Root
      Def Expr
        NamedNode expression
    ");
}

#[test]
fn reference() {
    let input = indoc! {r#"
    Expr = (identifier)
    Q = (call (Expr))
    "#};

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Expr
        NamedNode identifier
      Def Q
        NamedNode call
          Ref Expr
    ");
}

#[test]
fn alternation_unlabeled() {
    let res = Query::expect_valid_ast("Q = [(identifier) (number)]");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Alt
          Branch
            NamedNode identifier
          Branch
            NamedNode number
    ");
}

#[test]
fn alternation_tagged() {
    let input = indoc! {r#"
    Q = [Ident: (identifier) Num: (number)]
    "#};

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Alt
          Branch Ident:
            NamedNode identifier
          Branch Num:
            NamedNode number
    ");
}

#[test]
fn sequence() {
    let res = Query::expect_valid_ast("Q = {(a) (b) (c)}");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Seq
          NamedNode a
          NamedNode b
          NamedNode c
    ");
}

#[test]
fn quantifier_star() {
    let res = Query::expect_valid_ast("Q = (statement)*");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        QuantifiedExpr *
          NamedNode statement
    ");
}

#[test]
fn quantifier_plus() {
    let res = Query::expect_valid_ast("Q = (statement)+");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        QuantifiedExpr +
          NamedNode statement
    ");
}

#[test]
fn quantifier_optional() {
    let res = Query::expect_valid_ast("Q = (statement)?");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        QuantifiedExpr ?
          NamedNode statement
    ");
}

#[test]
fn quantifier_non_greedy() {
    let res = Query::expect_valid_ast("Q = (statement)*?");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        QuantifiedExpr *?
          NamedNode statement
    ");
}

#[test]
fn anchor() {
    let res = Query::expect_valid_ast("Q = (block . (statement))");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode block
          .
          NamedNode statement
    ");
}

#[test]
fn negated_field() {
    let res = Query::expect_valid_ast("Q = (function !async)");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode function
          NegatedField !async
    ");
}

#[test]
fn complex_example() {
    let input = indoc! {r#"
    Expression = [
        Ident: (identifier) @name :: string
        Binary: (binary_expression
            left: (Expression) @left
            right: (Expression) @right)
    ]
    "#};

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Expression
        Alt
          Branch Ident:
            CapturedExpr @name :: string
              NamedNode identifier
          Branch Binary:
            NamedNode binary_expression
              CapturedExpr @left
                FieldExpr left:
                  Ref Expression
              CapturedExpr @right
                FieldExpr right:
                  Ref Expression
    ");
}

#[test]
fn ast_with_errors() {
    let res = Query::expect_invalid("Q = (call (Undefined))");
    insta::assert_snapshot!(res, @r"
    error: `Undefined` is not defined
      |
    1 | Q = (call (Undefined))
      |            ^^^^^^^^^
    ");
}

#[test]
fn supertype() {
    let res = Query::expect_valid_ast("Q = (expression/binary_expression)");
    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        NamedNode expression
    ");
}

#[test]
fn multiple_fields() {
    let input = indoc! {r#"
    Q = (binary_expression
        left: (_) @left
        operator: _ @op
        right: (_) @right) @expr
    "#};

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        CapturedExpr @expr
          NamedNode binary_expression
            CapturedExpr @left
              FieldExpr left:
                NamedNode (any)
            CapturedExpr @op
              FieldExpr operator:
                AnonymousNode (any)
            CapturedExpr @right
              FieldExpr right:
                NamedNode (any)
    ");
}
