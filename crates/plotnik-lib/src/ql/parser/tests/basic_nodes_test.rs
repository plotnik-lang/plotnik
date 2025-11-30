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
      NamedNode
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
      NamedNode
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
      AnonNode
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
      AnonNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "function_definition"
        Field
          LowerIdent "name"
          Colon ":"
          NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "a"
        NamedNode
          ParenOpen "("
          LowerIdent "b"
          NamedNode
            ParenOpen "("
            LowerIdent "c"
            NamedNode
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
      NamedNode
        ParenOpen "("
        LowerIdent "block"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        NamedNode
          ParenOpen "("
          LowerIdent "statement"
          ParenClose ")"
        ParenClose ")"
    "#);
}
