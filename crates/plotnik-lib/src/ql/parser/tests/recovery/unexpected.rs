use crate::Query;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    "#);
}

#[test]
fn multiple_consecutive_garbage() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Error
          Garbage "^^^"
        Error
          Garbage "$$$"
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
    "#);
}

#[test]
fn garbage_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    "#);
}

#[test]
fn only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Error
          Garbage "^^^"
        Error
          Garbage "$$$"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "a"
          Capture
            Tree
              ParenOpen "("
              LowerIdent "b"
              ParenClose ")"
            At "@"
          Error
            At "@"
          Error
            At "@"
          Tree
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
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
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^
    error: unnamed definition must be last in file; add a name: `Name = (a (b) @@@ (c))`
      |
    1 | (a (b) @@@ (c)) (d)
      | ^^^^^^^^^^^^^^^
    "#);
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
        Error
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    "#);
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r##"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "a"
          Tree
            ParenOpen "("
            Error
              Predicate "#eq?"
            Error
              At "@"
            Error
              LowerIdent "x"
            Lit
              StringLit "\"foo\""
            ParenClose ")"
          Error
            LowerIdent "b"
          ParenClose ")"
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a (#eq? @x "foo") b)
      |          ^
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a (#eq? @x "foo") b)
      |           ^
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r##"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
      Def
        Error
          Predicate "#match?"
        Error
          At "@"
          LowerIdent "name"
      Def
        Lit
          StringLit "\"test\""
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier) #match? @name "test"
      | ^^^^^^^^^^^^
    "##);
}

#[test]
fn multiline_garbage_recovery() {
    let input = indoc! {r#"
    (a
    ^^^
    b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "a"
          Error
            Garbage "^^^"
          Error
            LowerIdent "b"
          ParenClose ")"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    2 | ^^^
      | ^^^
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    3 | b)
      | ^
    "#);
}

#[test]
fn capture_with_invalid_char() {
    let input = indoc! {r#"
    (identifier) @123
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
        Error
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Error
            Garbage "^^^"
          Error
            At "@"
          Error
            LowerIdent "name"
          BracketClose "]"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [^^^ @name]
      |  ^^^
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [^^^ @name]
      |      ^
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | [^^^ @name]
      |       ^^^^
    "#);
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    "#);
}
