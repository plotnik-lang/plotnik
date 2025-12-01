use crate::Query;
use indoc::indoc;

#[test]
fn error_node() {
    let input = indoc! {r#"
    (ERROR)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
    "#);
}

#[test]
fn error_node_with_capture() {
    let input = indoc! {r#"
    (ERROR) @err
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            KwError "ERROR"
            ParenClose ")"
          CaptureName "@err"
    "#);
}

#[test]
fn missing_node_bare() {
    let input = indoc! {r#"
    (MISSING)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_type() {
    let input = indoc! {r#"
    (MISSING identifier)
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          LowerIdent "identifier"
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_string() {
    let input = indoc! {r#"
    (MISSING ";")
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          StringLit "\";\""
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_with_capture() {
    let input = indoc! {r#"
    (MISSING ";") @missing_semi
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            StringLit "\";\""
            ParenClose ")"
          CaptureName "@missing_semi"
    "#);
}

#[test]
fn error_in_alternation() {
    let input = indoc! {r#"
    [(ERROR) (identifier)]
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Tree
            ParenOpen "("
            KwError "ERROR"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn missing_in_sequence() {
    let input = indoc! {r#"
    {(MISSING ";") (identifier)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            StringLit "\";\""
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn special_node_nested() {
    let input = indoc! {r#"
    (function_definition
        body: (block (ERROR)))
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "function_definition"
          Field
            LowerIdent "body"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "block"
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
    (ERROR)*
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
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
    (MISSING identifier)?
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier
          Tree
            ParenOpen "("
            KwMissing "MISSING"
            LowerIdent "identifier"
            ParenClose ")"
          Question "?"
    "#);
}
