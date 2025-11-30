use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Tree
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        Garbage "^^^"
      Tree
        ParenOpen "("
        LowerIdent "string"
        ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
      Tree
        ParenOpen "("
        LowerIdent "ok"
        ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ $$$ %%% (ok)
      |     ^^^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
      Tree
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ $$$
      | ^^^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Error
          Garbage "^^^"
        Tree
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BracketClose "]"
    ---
    error: unexpected token; expected a child expression or closing delimiter
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "a"
          Capture
            Tree
              ParenOpen "("
              LowerIdent "b"
              ParenClose ")"
            At "@"
        At "@"
      Error
        At "@"
      Tree
        ParenOpen "("
        LowerIdent "c"
        ParenClose ")"
      Error
        ParenClose ")"
      Tree
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
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (a (b) @@@ (c)) (d)
      |               ^
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
      Tree
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        XMLGarbage "</div>"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <div>(identifier)</div>
      | ^^^^^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
      Tree
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <br/> (a)
      | ^^^^^
    "#);
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    insta::assert_snapshot!(snapshot(input), @r##"
    Root
      Tree
        ParenOpen "("
        LowerIdent "a"
        Capture
          Tree
            ParenOpen "("
            Error
              Predicate "#eq?"
          At "@"
          LowerIdent "x"
        Lit
          StringLit "\"foo\""
        ParenClose ")"
      Tree
        LowerIdent "b"
      Error
        ParenClose ")"
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^
    error: expected closing ')' for tree
      |
    1 | (a (#eq? @x "foo") b)
      |          ^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (a (#eq? @x "foo") b)
      |                     ^
    "##);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    insta::assert_snapshot!(snapshot(input), @r##"
    Root
      Tree
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        Predicate "#match?"
      Error
        At "@"
      Tree
        LowerIdent "name"
      Lit
        StringLit "\"test\""
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^
    error: capture '@' must follow an expression to capture
      |
    1 | (identifier) #match? @name "test"
      |                      ^
    "##);
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
      Tree
        ParenOpen "("
        LowerIdent "a"
        Error
          Garbage "^^^"
        Tree
          LowerIdent "b"
        ParenClose ")"
    ---
    error: unexpected token; expected a child expression or closing delimiter
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
      Capture
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
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
      Tree
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "name"
          Colon ":"
          Error
            Garbage "%%%"
        ParenClose ")"
    ---
    error: unexpected token; expected an expression
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
      Capture
        Alt
          BracketOpen "["
          Error
            Garbage "^^^"
        At "@"
        LowerIdent "name"
      Error
        BracketClose "]"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [^^^ @name]
      |  ^^^
    error: expected closing ']' for alternation
      |
    1 | [^^^ @name]
      |      ^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | [^^^ @name]
      |           ^
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
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "Expr2"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
      Error
        Garbage "^^^"
      Def
        UpperIdent "B"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
      Error
        Garbage "$$$"
      Def
        UpperIdent "C"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    2 | ^^^
      | ^^^
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    4 | $$$
      | ^^^
    "#);
}
