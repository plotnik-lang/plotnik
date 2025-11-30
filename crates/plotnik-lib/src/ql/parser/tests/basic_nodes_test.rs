use super::helpers_test::*;
use indoc::indoc;

#[test]
fn empty_input() {
    insta::assert_snapshot!(snapshot(""), @"Root");
}

#[test]
fn simple_named_node() {
    let input = indoc! {r#"
    (identifier)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
    "#);
}

#[test]
fn wildcard() {
    let input = indoc! {r#"
    (_)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        Underscore "_"
        ParenClose ")"
    "#);
}

#[test]
fn anonymous_node() {
    let input = indoc! {r#"
    "if"
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Lit
        StringLit "\"if\""
    "#);
}

#[test]
fn anonymous_node_operator() {
    let input = indoc! {r#"
    "+="
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Lit
        StringLit "\"+=\""
    "#);
}

#[test]
fn nested_node() {
    let input = indoc! {r#"
    (function_definition name: (identifier))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn deeply_nested() {
    let input = indoc! {r#"
    (a
        (b
        (c
            (d))))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            Node
              ParenOpen "("
              LowerIdent "d"
              ParenClose ")"
            ParenClose ")"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn sibling_children() {
    let input = indoc! {r#"
    (block
        (statement)
        (statement)
        (statement))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "block"
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        ParenClose ")"
    "#);
}
