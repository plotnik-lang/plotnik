use crate::Query;
use indoc::indoc;

#[test]
fn empty_input() {
    insta::assert_snapshot!(Query::expect_valid_cst(""), @"Root");
}

#[test]
fn simple_named_node() {
    let input = indoc! {r#"
    Q = (identifier)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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
fn nested_node() {
    let input = indoc! {r#"
    Q = (function_definition name: (identifier))
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
fn deeply_nested() {
    let input = indoc! {r#"
    Q = (a
        (b
        (c
            (d))))
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "a"
          Tree
            ParenOpen "("
            Id "b"
            Tree
              ParenOpen "("
              Id "c"
              Tree
                ParenOpen "("
                Id "d"
                ParenClose ")"
              ParenClose ")"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn sibling_children() {
    let input = indoc! {r#"
    Q = (block
        (statement)
        (statement)
        (statement))
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "block"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "statement"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn wildcard() {
    let input = indoc! {r#"
    Q = (_)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Underscore "_"
          ParenClose ")"
    "#);
}

#[test]
fn anonymous_node() {
    let input = indoc! {r#"
    Q = "if"
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Str
          DoubleQuote "\""
          StrVal "if"
          DoubleQuote "\""
    "#);
}

#[test]
fn anonymous_node_operator() {
    let input = indoc! {r#"
    Q = "+="
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Str
          DoubleQuote "\""
          StrVal "+="
          DoubleQuote "\""
    "#);
}

#[test]
fn supertype_basic() {
    let input = indoc! {r#"
    Q = (expression/binary_expression)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          Id "binary_expression"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_with_string_subtype() {
    let input = indoc! {r#"
    Q = (expression/"()")
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          DoubleQuote "\""
          StrVal "()"
          DoubleQuote "\""
          ParenClose ")"
    "#);
}

#[test]
fn supertype_with_capture() {
    let input = indoc! {r#"
    Q = (expression/binary_expression) @expr
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
            Id "expression"
            Slash "/"
            Id "binary_expression"
            ParenClose ")"
          CaptureToken "@expr"
    "#);
}

#[test]
fn supertype_with_children() {
    let input = indoc! {r#"
    Q = (expression/binary_expression
        left: (_) @left
        right: (_) @right)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          Id "binary_expression"
          Capture
            Field
              Id "left"
              Colon ":"
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
            CaptureToken "@left"
          Capture
            Field
              Id "right"
              Colon ":"
              Tree
                ParenOpen "("
                Underscore "_"
                ParenClose ")"
            CaptureToken "@right"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_nested() {
    let input = indoc! {r#"
    Q = (statement/expression_statement
        (expression/call_expression))
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "statement"
          Slash "/"
          Id "expression_statement"
          Tree
            ParenOpen "("
            Id "expression"
            Slash "/"
            Id "call_expression"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_in_alternation() {
    let input = indoc! {r#"
    Q = [(expression/identifier) (expression/number)]
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
              Id "expression"
              Slash "/"
              Id "identifier"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "expression"
              Slash "/"
              Id "number"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn no_supertype_plain_node() {
    let input = indoc! {r#"
    Q = (identifier)
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
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
