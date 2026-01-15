//! Trivia (whitespace, comments) preservation tests.
//!
//! These tests use expect_valid_cst_full to verify trivia nodes are captured.

use crate::Query;
use indoc::indoc;

#[test]
fn whitespace_preserved() {
    let input = indoc! {r#"
    Q = (identifier)  @name
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Capture
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          Whitespace "  "
          CaptureToken "@name"
      Newline "\n"
    "#);
}

#[test]
fn comment_preserved() {
    let input = indoc! {r#"
    // comment
    Q = (identifier)
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      LineComment "// comment"
      Newline "\n"
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn comment_inside_expression() {
    let input = indoc! {r#"
    Q = (call // inline
        name: (identifier))
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Tree
          ParenOpen "("
          Id "call"
          Whitespace " "
          LineComment "// inline"
          Newline "\n"
          Whitespace "    "
          Field
            Id "name"
            Colon ":"
            Whitespace " "
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn trivia_filtered_by_default() {
    let input = indoc! {r#"
    // comment
    Q = (identifier)
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      LineComment "// comment"
      Newline "\n"
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Newline "\n"
    "#);
}

#[test]
fn trivia_between_alternation_items() {
    let input = indoc! {r#"
    Q = [
        (a)
        (b)
    ]
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Alt
          BracketOpen "["
          Newline "\n"
          Whitespace "    "
          Branch
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Newline "\n"
          Whitespace "    "
          Branch
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
          Newline "\n"
          BracketClose "]"
      Newline "\n"
    "#);
}

#[test]
fn whitespace_only() {
    let input = "    ";

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Whitespace "    "
    "#);
}

#[test]
fn comment_only_raw() {
    let input = indoc! {r#"
    // just a comment
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      LineComment "// just a comment"
      Newline "\n"
    "#);
}

#[test]
fn semicolon_comment() {
    let input = indoc! {r#"
    ; semicolon comment
    Q = (identifier)
    "#};

    let res = Query::expect_valid_cst_full(input);

    insta::assert_snapshot!(res, @r#"
    Root
      LineComment "; semicolon comment"
      Newline "\n"
      Def
        Id "Q"
        Whitespace " "
        Equals "="
        Whitespace " "
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Newline "\n"
    "#);
}
