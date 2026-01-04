use crate::Query;
use indoc::indoc;

#[test]
fn error_node() {
    let input = indoc! {r#"
    Q = (ERROR)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
    "#);
}

#[test]
fn error_node_with_capture() {
    let input = indoc! {r#"
    Q = (ERROR) @err
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Tree
            ParenOpen "("
            KwError "ERROR"
            ParenClose ")"
          CaptureToken "@err"
    "#);
}

#[test]
fn missing_node_bare() {
    let input = indoc! {r#"
    Q = (MISSING)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_type() {
    let input = indoc! {r#"
    Q = (MISSING identifier)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          Id "identifier"
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_string() {
    let input = indoc! {r#"
    Q = (MISSING ";")
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          DoubleQuote "\""
          StrVal ";"
          DoubleQuote "\""
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_capture() {
    let input = indoc! {r#"
    Q = (MISSING ";") @missing_semi
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Capture
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            DoubleQuote "\""
            StrVal ";"
            DoubleQuote "\""
            ParenClose ")"
          CaptureToken "@missing_semi"
    "#);
}

#[test]
fn error_in_alternation() {
    let input = indoc! {r#"
    Q = [(ERROR) (identifier)]
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              KwError "ERROR"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn missing_in_sequence() {
    let input = indoc! {r#"
    Q = {(MISSING ";") (identifier)}
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            DoubleQuote "\""
            StrVal ";"
            DoubleQuote "\""
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn special_node_nested() {
    let input = indoc! {r#"
    Q = (function_definition
        body: (block (ERROR)))
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "function_definition"
          Field
            Id "body"
            Colon ":"
            Tree
              ParenOpen "("
              Id "block"
              Tree
                ParenOpen "("
                KwError "ERROR"
                ParenClose ")"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn error_with_quantifier() {
    let input = indoc! {r#"
    Q = (ERROR)*
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Quantifier
          Tree
            ParenOpen "("
            KwError "ERROR"
            ParenClose ")"
          Star "*"
    "#);
}

#[test]
fn missing_with_quantifier() {
    let input = indoc! {r#"
    Q = (MISSING identifier)?
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Quantifier
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            Id "identifier"
            ParenClose ")"
          Question "?"
    "#);
}
