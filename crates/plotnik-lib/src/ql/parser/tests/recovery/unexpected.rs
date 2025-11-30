use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
        Error
          Garbage "^^^"
      Def
        Tree
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (identifier) ^^^ (string)
      |              ^^^
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier) ^^^ (string)
      | ^^^^^^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = ^^^`
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
      Def
        Error
          Garbage "^^^"
      Def
        Error
          Garbage "$$$"
      Def
        Error
          Garbage "%%%"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = ^^^`
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^
    error: unnamed definition must be last in file; add a name: `Name = $$$`
      |
    1 | ^^^ $$$ %%% (ok)
      |     ^^^
    error: unnamed definition must be last in file; add a name: `Name = %%%`
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
      Def
        Error
          Garbage "^^^"
      Def
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ (a)
      | ^^^
    error: unnamed definition must be last in file; add a name: `Name = ^^^`
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
      Def
        Error
          Garbage "^^^"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = ^^^`
      |
    1 | ^^^ $$$
      | ^^^
    "#);
}

#[test]
fn garbage_inside_alternation() {
    let input = indoc! {r#"
    [(a) ^^^ (b)]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
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
      Def
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
      Def
        Error
          At "@"
      Def
        Tree
          ParenOpen "("
          LowerIdent "c"
          ParenClose ")"
      Def
        Error
          ParenClose ")"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = (a (b) @@`
      |
    1 | (a (b) @@@ (c)) (d)
      | ^^^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = @`
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^
    error: unnamed definition must be last in file; add a name: `Name = (c)`
      |
    1 | (a (b) @@@ (c)) (d)
      |            ^^^
    error: unnamed definition must be last in file; add a name: `Name = )`
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
      Def
        Error
          XMLGarbage "<div>"
      Def
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = <div>`
      |
    1 | <div>(identifier)</div>
      | ^^^^^
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | <div>(identifier)</div>
      |      ^^^^^^^^^^^^
    "#);
}

#[test]
fn xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Error
          XMLGarbage "<br/>"
      Def
        Tree
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <br/> (a)
      | ^^^^^
    error: unnamed definition must be last in file; add a name: `Name = <br/>`
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
      Def
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
      Def
        Tree
          LowerIdent "b"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = (a (#eq? @x "foo")`
      |
    1 | (a (#eq? @x "foo") b)
      | ^^^^^^^^^^^^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = b`
      |
    1 | (a (#eq? @x "foo") b)
      |                    ^
    "##);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    insta::assert_snapshot!(snapshot(input), @r##"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
        Error
          Predicate "#match?"
      Def
        Error
          At "@"
      Def
        Tree
          LowerIdent "name"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier) #match? @name "test"
      | ^^^^^^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = #match?`
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = @`
      |
    1 | (identifier) #match? @name "test"
      |                      ^
    error: unnamed definition must be last in file; add a name: `Name = name`
      |
    1 | (identifier) #match? @name "test"
      |                       ^^^^
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
      Def
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
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          At "@"
      Def
        Error
          Garbage "123"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (identifier) @123
      |               ^^^
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @`
      |
    1 | (identifier) @123
      | ^^^^^^^^^^^^^^
    "#);
}

#[test]
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
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
      Def
        Capture
          Alt
            BracketOpen "["
            Error
              Garbage "^^^"
          At "@"
          LowerIdent "name"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = [^^^ @name`
      |
    1 | [^^^ @name]
      | ^^^^^^^^^^
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
      Def
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
      Def
        Error
          Garbage "^^^"
      Def
        UpperIdent "B"
        Equals "="
        Tree
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
      Def
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
    error: unnamed definition must be last in file; add a name: `Name = ^^^`
      |
    2 | ^^^
      | ^^^
    "#);
}
