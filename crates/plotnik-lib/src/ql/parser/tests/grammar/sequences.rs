use crate::Query;
use indoc::indoc;

#[test]
fn simple_sequence() {
    let input = indoc! {r#"
    {(a) (b)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    {}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_single_element() {
    let input = indoc! {r#"
    {(identifier)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    {(comment)* @comments (function) @fn}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Capture
            Quantifier
              Tree
                ParenOpen "("
                Id "comment"
                ParenClose ")"
              Star "*"
            At "@"
            Id "comments"
          Capture
            Tree
              ParenOpen "("
              Id "function"
              ParenClose ")"
            At "@"
            Id "fn"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_quantifier() {
    let input = indoc! {r#"
    {(a) (b)}+
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    {{(a)} {(b)}}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    (block {(statement) (statement)})
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    {[(a) (b)] (c)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    let input = indoc! {r#"
    {(number) {"," (number)}*}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    let input = indoc! {r#"
    {. (first) (second) .}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
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
    "#);
}
