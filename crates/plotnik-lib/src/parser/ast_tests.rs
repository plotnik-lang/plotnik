use crate::Query;
use indoc::indoc;

#[test]
fn simple_tree() {
    let query = Query::try_from("Q = (identifier)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        NamedNode function_definition
          FieldExpr name:
            NamedNode identifier
    ");
}

#[test]
fn wildcard() {
    let query = Query::try_from("Q = (_)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        NamedNode (any)
    ");
}

#[test]
fn literal() {
    let query = Query::try_from(r#"Q = "if""#).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r#"
    Root
      Def Q
        AnonymousNode "if"
    "#);
}

#[test]
fn capture() {
    let query = Query::try_from("Q = (identifier) @name").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        CapturedExpr @name
          NamedNode identifier
    ");
}

#[test]
fn capture_with_type() {
    let query = Query::try_from("Q = (identifier) @name :: string").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        CapturedExpr @name :: string
          NamedNode identifier
    ");
}

#[test]
fn named_definition() {
    let query = Query::try_from("Expr = (expression)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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
    let query = Query::try_from("Q = [(identifier) (number)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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
    let query = Query::try_from("Q = {(a) (b) (c)}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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
    let query = Query::try_from("Q = (statement)*").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        QuantifiedExpr *
          NamedNode statement
    ");
}

#[test]
fn quantifier_plus() {
    let query = Query::try_from("Q = (statement)+").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        QuantifiedExpr +
          NamedNode statement
    ");
}

#[test]
fn quantifier_optional() {
    let query = Query::try_from("Q = (statement)?").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        QuantifiedExpr ?
          NamedNode statement
    ");
}

#[test]
fn quantifier_non_greedy() {
    let query = Query::try_from("Q = (statement)*?").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        QuantifiedExpr *?
          NamedNode statement
    ");
}

#[test]
fn anchor() {
    let query = Query::try_from("Q = (block . (statement))").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        NamedNode block
          .
          NamedNode statement
    ");
}

#[test]
fn negated_field() {
    let query = Query::try_from("Q = (function !async)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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
    let query = Query::try_from("Q = (call (Undefined))").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: `Undefined` is not defined
      |
    1 | Q = (call (Undefined))
      |            ^^^^^^^^^
    ");
}

#[test]
fn supertype() {
    let query = Query::try_from("Q = (expression/binary_expression)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
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
