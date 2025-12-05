use crate::Query;
use indoc::indoc;

#[test]
fn simple_tree() {
    let query = Query::try_from("(identifier)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        NamedNode identifier
    ");
}

#[test]
fn nested_tree() {
    let input = indoc! {r#"
    (function_definition name: (identifier))
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        NamedNode function_definition
          FieldExpr name:
            NamedNode identifier
    ");
}

#[test]
fn wildcard() {
    let query = Query::try_from("(_)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        NamedNode (any)
    ");
}

#[test]
fn literal() {
    let query = Query::try_from(r#""if""#).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r#"
    Root
      Def
        AnonymousNode "if"
    "#);
}

#[test]
fn capture() {
    let query = Query::try_from("(identifier) @name").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        CapturedExpr @name
          NamedNode identifier
    ");
}

#[test]
fn capture_with_type() {
    let query = Query::try_from("(identifier) @name :: string").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
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
    (call (Expr))
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Expr
        NamedNode identifier
      Def
        NamedNode call
          Ref Expr
    ");
}

#[test]
fn alternation_unlabeled() {
    let query = Query::try_from("[(identifier) (number)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
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
    [Ident: (identifier) Num: (number)]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        Alt
          Branch Ident:
            NamedNode identifier
          Branch Num:
            NamedNode number
    ");
}

#[test]
fn sequence() {
    let query = Query::try_from("{(a) (b) (c)}").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        Seq
          NamedNode a
          NamedNode b
          NamedNode c
    ");
}

#[test]
fn quantifier_star() {
    let query = Query::try_from("(statement)*").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        QuantifiedExpr *
          NamedNode statement
    ");
}

#[test]
fn quantifier_plus() {
    let query = Query::try_from("(statement)+").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        QuantifiedExpr +
          NamedNode statement
    ");
}

#[test]
fn quantifier_optional() {
    let query = Query::try_from("(statement)?").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        QuantifiedExpr ?
          NamedNode statement
    ");
}

#[test]
fn quantifier_non_greedy() {
    let query = Query::try_from("(statement)*?").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        QuantifiedExpr *?
          NamedNode statement
    ");
}

#[test]
fn anchor() {
    let query = Query::try_from("(block . (statement))").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        NamedNode block
          .
          NamedNode statement
    ");
}

#[test]
fn negated_field() {
    let query = Query::try_from("(function !async)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
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
    let query = Query::try_from("(call (Undefined))").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: undefined reference: `Undefined`
      |
    1 | (call (Undefined))
      |        ^^^^^^^^^ undefined reference: `Undefined`
    "#);
}

#[test]
fn supertype() {
    let query = Query::try_from("(expression/binary_expression)").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        NamedNode expression
    ");
}

#[test]
fn multiple_fields() {
    let input = indoc! {r#"
    (binary_expression
        left: (_) @left
        operator: _ @op
        right: (_) @right) @expr
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
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
