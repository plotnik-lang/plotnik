use crate::Query;
use indoc::indoc;

#[test]
fn simple_sequence() {
    let input = indoc! {r#"
    {(a) (b)}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "identifier"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Capture
            Quantifier
              Tree
                ParenOpen "("
                LowerIdent "comment"
                ParenClose ")"
              Star "*"
            CaptureName "@comments"
          Capture
            Tree
              ParenOpen "("
              LowerIdent "function"
              ParenClose ")"
            CaptureName "@fn"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_with_quantifier() {
    let input = indoc! {r#"
    {(a) (b)}+
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Quantifier
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
            Tree
              ParenOpen "("
              LowerIdent "b"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
            BraceClose "}"
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              LowerIdent "b"
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

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "block"
          Seq
            BraceOpen "{"
            Tree
              ParenOpen "("
              LowerIdent "statement"
              ParenClose ")"
            Tree
              ParenOpen "("
              LowerIdent "statement"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Alt
            BracketOpen "["
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
            Tree
              ParenOpen "("
              LowerIdent "b"
              ParenClose ")"
            BracketClose "]"
          Tree
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
          BraceClose "}"
    "#);
}

#[test]
fn sequence_comma_separated_pattern() {
    let input = indoc! {r#"
    {(number) {"," (number)}*}
    "#};

    let query = Query::new(input);
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Tree
            ParenOpen "("
            LowerIdent "number"
            ParenClose ")"
          Quantifier
            Seq
              BraceOpen "{"
              Lit
                StringLit "\",\""
              Tree
                ParenOpen "("
                LowerIdent "number"
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
    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Seq
          BraceOpen "{"
          Anchor
            Dot "."
          Tree
            ParenOpen "("
            LowerIdent "first"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "second"
            ParenClose ")"
          Anchor
            Dot "."
          BraceClose "}"
    "#);
}
