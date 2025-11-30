use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        Garbage "^^^"
      Node
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | (identifier) ^^^ (string)
      |              ^^^
    "#);
}

#[test]
fn multiple_consecutive_garbage() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        Garbage "^^^"
      Error
        Garbage "$$$"
      Error
        Garbage "%%%"
      Node
        ParenOpen "("
        LowerIdent "ok"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ $$$ %%% (ok)
      |     ^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ $$$ %%% (ok)
      |         ^^^
    "#);
}

#[test]
fn garbage_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        Garbage "^^^"
      Node
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ (a)
      | ^^^
    "#);
}

#[test]
fn only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        Garbage "^^^"
      Error
        Garbage "$$$"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ $$$
      | ^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | ^^^ $$$
      |     ^^^
    "#);
}

#[test]
fn garbage_inside_alternation() {
    let input = indoc! {r#"
    [(a) ^^^ (b)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Error
          Garbage "^^^"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BracketClose "]"
    ---
    error: unexpected token inside node; expected a child pattern or closing delimiter
      |
    1 | [(a) ^^^ (b)]
      |      ^^^
    "#);
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        Capture
          At "@"
        Capture
          At "@"
        Capture
          At "@"
        Node
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
        ParenClose ")"
      Node
        ParenOpen "("
        LowerIdent "d"
        ParenClose ")"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (a (b) @@@ (c)) (d)
      |         ^
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (a (b) @@@ (c)) (d)
      |            ^
    "#);
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        XMLGarbage "<div>"
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        XMLGarbage "</div>"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | <div>(identifier)</div>
      | ^^^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | <div>(identifier)</div>
      |                  ^^^^^^
    "#);
}

#[test]
fn xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        XMLGarbage "<br/>"
      Node
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | <br/> (a)
      | ^^^^^
    "#);
}

#[test]
fn multiline_garbage_recovery() {
    let input = indoc! {r#"
    (a
    ^^^
    b)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Error
          Garbage "^^^"
        Node
          LowerIdent "b"
        ParenClose ")"
    ---
    error: unexpected token inside node; expected a child pattern or closing delimiter
      |
    2 | ^^^
      | ^^^
    "#);
}

#[test]
fn capture_with_invalid_char() {
    let input = indoc! {r#"
    (identifier) @123
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
      Error
        Garbage "123"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (identifier) @123
      |               ^^^
    "#);
}

#[test]
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "name"
          Colon ":"
          Error
            Garbage "%%%"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern
      |
    1 | (call name: %%%)
      |             ^^^
    "#);
}

#[test]
fn alternation_recovery_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Error
          Garbage "^^^"
        Capture
          At "@"
          LowerIdent "name"
        BracketClose "]"
    ---
    error: unexpected token inside node; expected a child pattern or closing delimiter
      |
    1 | [^^^ @name]
      |  ^^^
    "#);
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "Expr"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "Expr2"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    1 | Expr = (a) ^^^ Expr2 = (b)
      |            ^^^
    "#);
}

#[test]
fn multiple_definitions_with_garbage_between() {
    let input = indoc! {r#"
    A = (a)
    ^^^
    B = (b)
    $$$
    C = (c)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        UpperIdent "A"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "B"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
      Error
        Garbage "$$$"
      Def
        UpperIdent "C"
        Equals "="
        Node
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
    ---
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    2 | ^^^
      | ^^^
    error: unexpected token; expected a pattern like (node), [choice], {sequence}, "literal", @capture, or _
      |
    4 | $$$
      | ^^^
    "#);
}