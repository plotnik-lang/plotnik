use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn whitespace_preserved() {
    let input = indoc! {r#"
    (identifier)  @name
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Whitespace "  "
      Capture
        At "@"
        LowerIdent "name"
      Newline "\n"
    "#);
}

#[test]
fn comment_preserved() {
    let input = indoc! {r#"
    // comment
    (identifier)
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      LineComment "// comment"
      Newline "\n"
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn multiline() {
    let input = indoc! {r#"
    (a)

    (b)
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
      Newline "\n"
      Newline "\n"
      Node
        ParenOpen "("
        LowerIdent "b"
        ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn comment_inside_pattern() {
    let input = indoc! {r#"
    (call // inline
        name: (identifier))
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Whitespace " "
        LineComment "// inline"
        Newline "\n"
        Whitespace "    "
        Field
          LowerIdent "name"
          Colon ":"
          Whitespace " "
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn trivia_filtered_by_default() {
    let input = indoc! {r#"
    // comment
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
fn trivia_between_alternation_items() {
    let input = indoc! {r#"
    [
        (a)
        (b)
    ]
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      Alt
        BracketOpen "["
        Newline "\n"
        Whitespace "    "
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Newline "\n"
        Whitespace "    "
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BracketClose "]"
      Newline "\n"
      Newline "\n"
    "#);
}

#[test]
fn whitespace_only() {
    let input = "    ";

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      Whitespace "    "
    "#);
}

#[test]
fn comment_only_raw() {
    let input = indoc! {r#"
    // just a comment
    "#};

    insta::assert_snapshot!(snapshot_raw(input), @r#"
    Root
      LineComment "// just a comment"
      Newline "\n"
    "#);
}