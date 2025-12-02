use crate::Query;
use indoc::indoc;

#[test]
fn simple_tree() {
    let query = Query::new("(identifier)");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree identifier
    "#);
}

#[test]
fn nested_tree() {
    let input = indoc! {r#"
    (function_definition name: (identifier))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree function_definition
          Field name:
            Tree identifier
    "#);
}

#[test]
fn wildcard() {
    let query = Query::new("(_)");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree _
    "#);
}

#[test]
fn literal() {
    let query = Query::new(r#""if""#);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Str "if"
    "#);
}

#[test]
fn capture() {
    let query = Query::new("(identifier) @name");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture @name
          Tree identifier
    "#);
}

#[test]
fn capture_with_type() {
    let query = Query::new("(identifier) @name :: string");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture @name :: string
          Tree identifier
    "#);
}

#[test]
fn named_definition() {
    let query = Query::new("Expr = (expression)");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def Expr
        Tree expression
    "#);
}

#[test]
fn reference() {
    let input = indoc! {r#"
    Expr = (identifier)
    (call (Expr))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def Expr
        Tree identifier
      Def
        Tree call
          Ref Expr
    "#);
}

#[test]
fn alternation_unlabeled() {
    let query = Query::new("[(identifier) (number)]");
    insta::assert_snapshot!(query.snapshot_ast(), @r"
    Root
      Def
        Alt
          Branch
            Tree identifier
          Branch
            Tree number
    ");
}

#[test]
fn alternation_tagged() {
    let input = indoc! {r#"
    [Ident: (identifier) Num: (number)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r"
    Root
      Def
        Alt
          Branch Ident:
            Tree identifier
          Branch Num:
            Tree number
    ");
}

#[test]
fn sequence() {
    let query = Query::new("{(a) (b) (c)}");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          Tree a
          Tree b
          Tree c
    "#);
}

#[test]
fn quantifier_star() {
    let query = Query::new("(statement)*");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier *
          Tree statement
    "#);
}

#[test]
fn quantifier_plus() {
    let query = Query::new("(statement)+");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier +
          Tree statement
    "#);
}

#[test]
fn quantifier_optional() {
    let query = Query::new("(statement)?");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier ?
          Tree statement
    "#);
}

#[test]
fn quantifier_non_greedy() {
    let query = Query::new("(statement)*?");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier *?
          Tree statement
    "#);
}

#[test]
fn anchor() {
    let query = Query::new("(block . (statement))");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree block
          Anchor
          Tree statement
    "#);
}

#[test]
fn negated_field() {
    let query = Query::new("(function !async)");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree function
          NegatedField !async
    "#);
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

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r"
    Root
      Def Expression
        Alt
          Branch Ident:
            Capture @name :: string
              Tree identifier
          Branch Binary:
            Tree binary_expression
              Capture @left
                Field left:
                  Ref Expression
              Capture @right
                Field right:
                  Ref Expression
    ");
}

#[test]
fn ast_with_errors() {
    let query = Query::new("(call (Undefined))");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree call
          Ref Undefined
    ---
    error: undefined reference: `Undefined`
      |
    1 | (call (Undefined))
      |        ^^^^^^^^^ undefined reference: `Undefined`
    "#);
}

#[test]
fn supertype() {
    let query = Query::new("(expression/binary_expression)");
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree expression
    "#);
}

#[test]
fn multiple_fields() {
    let input = indoc! {r#"
    (binary_expression
        left: (_) @left
        operator: _ @op
        right: (_) @right) @expr
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r"
    Root
      Def
        Capture @expr
          Tree binary_expression
            Capture @left
              Field left:
                Tree _
            Capture @op
              Field operator:
                Wildcard
            Capture @right
              Field right:
                Tree _
    ");
}
