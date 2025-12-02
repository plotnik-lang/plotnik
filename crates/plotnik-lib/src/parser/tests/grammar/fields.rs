use crate::Query;
use indoc::indoc;

#[test]
fn field_expression() {
    let input = indoc! {r#"
    (call function: (identifier))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "call"
          Field
            Id "function"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn multiple_fields() {
    let input = indoc! {r#"
    (assignment
        left: (identifier)
        right: (expression))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "assignment"
          Field
            Id "left"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          Field
            Id "right"
            Colon ":"
            Tree
              ParenOpen "("
              Id "expression"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn negated_field() {
    let input = indoc! {r#"
    (function !async)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "function"
          NegatedField
            Negation "!"
            Id "async"
          ParenClose ")"
    "#);
}

#[test]
fn negated_and_regular_fields() {
    let input = indoc! {r#"
    (function
        !async
        name: (identifier))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "function"
          NegatedField
            Negation "!"
            Id "async"
          Field
            Id "name"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn mixed_children_and_fields() {
    let input = indoc! {r#"
    (if
        condition: (expr)
        (then_block)
        else: (else_block))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "if"
          Field
            Id "condition"
            Colon ":"
            Tree
              ParenOpen "("
              Id "expr"
              ParenClose ")"
          Tree
            ParenOpen "("
            Id "then_block"
            ParenClose ")"
          Field
            Id "else"
            Colon ":"
            Tree
              ParenOpen "("
              Id "else_block"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn fields_and_quantifiers() {
    let input = indoc! {r#"
    (node
        foo: (foo)?
        foo: (foo)??
        bar: (bar)*
        bar: (bar)*?
        baz: (baz)+?
        baz: (baz)+?)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Quantifier
            Field
              Id "foo"
              Colon ":"
              Tree
                ParenOpen "("
                Id "foo"
                ParenClose ")"
            Question "?"
          Quantifier
            Field
              Id "foo"
              Colon ":"
              Tree
                ParenOpen "("
                Id "foo"
                ParenClose ")"
            QuestionQuestion "??"
          Quantifier
            Field
              Id "bar"
              Colon ":"
              Tree
                ParenOpen "("
                Id "bar"
                ParenClose ")"
            Star "*"
          Quantifier
            Field
              Id "bar"
              Colon ":"
              Tree
                ParenOpen "("
                Id "bar"
                ParenClose ")"
            StarQuestion "*?"
          Quantifier
            Field
              Id "baz"
              Colon ":"
              Tree
                ParenOpen "("
                Id "baz"
                ParenClose ")"
            PlusQuestion "+?"
          Quantifier
            Field
              Id "baz"
              Colon ":"
              Tree
                ParenOpen "("
                Id "baz"
                ParenClose ")"
            PlusQuestion "+?"
          ParenClose ")"
    "#);
}

#[test]
fn fields_with_quantifiers_and_captures() {
    let input = indoc! {r#"
    (node foo: (bar)* @baz)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Capture
            Quantifier
              Field
                Id "foo"
                Colon ":"
                Tree
                  ParenOpen "("
                  Id "bar"
                  ParenClose ")"
              Star "*"
            At "@"
            Id "baz"
          ParenClose ")"
    "#);
}
