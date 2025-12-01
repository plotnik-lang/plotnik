use crate::Query;
use indoc::indoc;

#[test]
fn field_pattern() {
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
fn multiple_patterns_with_captures() {
    let input = indoc! {r#"
    (function) @func
    (class) @cls
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "function"
            ParenClose ")"
          At "@"
          Id "func"
      Def
        Capture
          Tree
            ParenOpen "("
            Id "class"
            ParenClose ")"
          At "@"
          Id "cls"
    ---
    error: unnamed definition must be last in file; add a name: `Name = (function) @func`
      |
    1 | (function) @func
      | ^^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (function) @func`
    "#);
}
