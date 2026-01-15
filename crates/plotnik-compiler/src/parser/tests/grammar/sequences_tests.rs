use crate::Query;
use indoc::indoc;

// Tree-sitter compatibility: ((a) (b)) parses as Seq, not as wildcard Tree with children

#[test]
fn treesitter_sequence_parses_as_seq() {
    // Tree-sitter style ((a) (b)) should produce Seq, same structure as {(a) (b)}
    let input = "Q = ((a) (b))";

    let res = Query::expect_cst_with_warnings(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          ParenOpen "("
          Tree
            ParenOpen "("
            Id "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn treesitter_single_item_sequence_parses_as_seq() {
    // Regression test: ((x)) should be Seq containing x, not wildcard Tree with child x
    let input = "Q = ((expression_statement))";

    let res = Query::expect_cst_with_warnings(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          ParenOpen "("
          Tree
            ParenOpen "("
            Id "expression_statement"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn named_node_with_child_remains_tree() {
    // (foo (bar)) is a named node with child, NOT a sequence
    let input = "Q = (foo (bar))";

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "foo"
          Tree
            ParenOpen "("
            Id "bar"
            ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn simple_sequence() {
    let input = indoc! {r#"
    Q = {(a) (b)}
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
            Id "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            Id "b"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn empty_sequence() {
    let input = indoc! {r#"
    Q = {}
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: empty `{}` is not allowed
      |
    1 | Q = {}
      |     ^^
      |
    help: sequences must contain at least one expression
    ");
}

#[test]
fn sequence_single_element() {
    let input = indoc! {r#"
    Q = {(identifier)}
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
            Id "identifier"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_captures() {
    let input = indoc! {r#"
    Q = {(comment)* @comments (function) @fn}
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Capture
            Quantifier
              Tree
                ParenOpen "("
                Id "comment"
                ParenClose ")"
              Star "*"
            CaptureToken "@comments"
          Capture
            Tree
              ParenOpen "("
              Id "function"
              ParenClose ")"
            CaptureToken "@fn"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_quantifier() {
    // Note: This tests parser behavior only. The pattern `{(a) (b)}+` is
    // semantically invalid (multi-element sequence without captures), but
    // we're only testing that the parser produces the correct CST.
    let input = indoc! {r#"
    Q = {(a) (b)}+
    "#};

    let res = Query::parse_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Quantifier
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
            BraceClose "}"
          Plus "+"
    "#);
}

#[test]
fn nested_sequences() {
    let input = indoc! {r#"
    Q = {{(a)} {(b)}}
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
            BraceClose "}"
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
            BraceClose "}"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_in_named_node() {
    let input = indoc! {r#"
    Q = (block {(statement) (statement)})
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
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "statement"
              ParenClose ")"
            BraceClose "}"
          ParenClose ")"
    "#);
}

#[test]
fn sequence_with_alternation() {
    let input = indoc! {r#"
    Q = {[(a) (b)] (c)}
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Alt
            BracketOpen "["
            Branch
              Tree
                ParenOpen "("
                Id "a"
                ParenClose ")"
            Branch
              Tree
                ParenOpen "("
                Id "b"
                ParenClose ")"
            BracketClose "]"
          Tree
            ParenOpen "("
            Id "c"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_comma_separated_expression() {
    // Note: This tests parser behavior only. The inner pattern `{"," (number)}*`
    // is semantically invalid (multi-element sequence without captures), but
    // we're only testing that the parser produces the correct CST.
    let input = indoc! {r#"
    Q = {(number) {"," (number)}*}
    "#};

    let res = Query::parse_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            Id "number"
            ParenClose ")"
          Quantifier
            Seq
              BraceOpen "{"
              Str
                DoubleQuote "\""
                StrVal ","
                DoubleQuote "\""
              Tree
                ParenOpen "("
                Id "number"
                ParenClose ")"
              BraceClose "}"
            Star "*"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_anchor() {
    // Boundary anchors require parent node context
    let input = indoc! {r#"
    Q = (parent {. (first) (second) .})
    "#};

    let res = Query::expect_valid_cst(input);

    insta::assert_snapshot!(res, @r#"
    Root
      Def
        Id "Q"
        Equals "="
        Tree
          ParenOpen "("
          Id "parent"
          Seq
            BraceOpen "{"
            Anchor
              Dot "."
            Tree
              ParenOpen "("
              Id "first"
              ParenClose ")"
            Tree
              ParenOpen "("
              Id "second"
              ParenClose ")"
            Anchor
              Dot "."
            BraceClose "}"
          ParenClose ")"
    "#);
}
