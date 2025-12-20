use crate::Query;
use indoc::indoc;

#[test]
fn whitespace_preserved() {
    let input = indoc! {r#"
    Q = (identifier)  @name
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
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
          At "@"
          Id "name"
      Newline "\n"
    "#);
}

#[test]
fn comment_preserved() {
    let input = indoc! {r#"
    // comment
    Q = (identifier)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      Whitespace "    "
    "#);
}

#[test]
fn comment_only_raw() {
    let input = indoc! {r#"
    // just a comment
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      LineComment "// just a comment"
      Newline "\n"
    "#);
}
