use crate::Query;
use indoc::indoc;

#[test]
fn whitespace_preserved() {
    let input = indoc! {r#"
    (identifier)  @name
    "#};

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
      Whitespace "  "
      Newline "\n"
    "#);
}

#[test]
fn comment_preserved() {
    let input = indoc! {r#"
    // comment
    (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      LineComment "// comment"
      Newline "\n"
      Def
        Tree
          ParenOpen "("
          Id "identifier"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unnamed definition must be last in file; add a name: `Name = (a)`
      |
    1 | (a)
      | ^^^ unnamed definition must be last in file; add a name: `Name = (a)`
    "#);
}

#[test]
fn comment_inside_expression() {
    let input = indoc! {r#"
    (call // inline
        name: (identifier))
    "#};

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      Def
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
    (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
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

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      Def
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
          BracketClose "]"
      Newline "\n"
      Newline "\n"
    "#);
}

#[test]
fn whitespace_only() {
    let input = "    ";

    let query = Query::new(input).unwrap();
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

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst_full(), @r#"
    Root
      LineComment "// just a comment"
      Newline "\n"
    "#);
}
