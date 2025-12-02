use crate::Query;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Error
          Garbage "^^^"
      Def
        Tree
          ParenOpen "("
          Id "string"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (identifier) ^^^ (string)
      |              ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier) ^^^ (string)
      | ^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier)`
    "#);
}

#[test]
fn multiple_consecutive_garbage() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
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
          Id "ok"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ $$$ %%% (ok)
      | ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn garbage_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Error
          Garbage "^^^"
      Def
        Tree
          ParenOpen "("
          Id "a"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | ^^^ (a)
      | ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
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
      | ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn garbage_inside_alternation() {
    let input = indoc! {r#"
    [(a) ^^^ (b)]
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Error
            Garbage "^^^"
          Branch
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
          BracketClose "]"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [(a) ^^^ (b)]
      |      ^^^ unexpected token; expected a child expression or closing delimiter
    "#);
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Capture
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
            At "@"
          Error
            At "@"
          Error
            At "@"
          Tree
            ParenOpen "("
            Id "c"
            ParenClose ")"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "d"
          ParenClose ")"
    ---
    error: expected capture name after '@'
      |
    1 | (a (b) @@@ (c)) (d)
      |         ^ expected capture name after '@'
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a (b) @@@ (c)) (d)
      |          ^ unexpected token; expected a child expression or closing delimiter
    error: unnamed definition must be last in file; add a name: `Name = (a (b) @@@ (c))`
      |
    1 | (a (b) @@@ (c)) (d)
      | ^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (a (b) @@@ (c))`
    "#);
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Error
          XMLGarbage "<div>"
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Error
          XMLGarbage "</div>"
        Error
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <div>(identifier)</div>
      | ^^^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <div>(identifier)</div>
      |                  ^^^^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Error
          XMLGarbage "<br/>"
      Def
        Tree
          ParenOpen "("
          Id "a"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | <br/> (a)
      | ^^^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn predicate_unsupported() {
    let input = indoc! {r#"
    (a (#eq? @x "foo") b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r##"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Tree
            ParenOpen "("
            Error
              Predicate "#eq?"
            Error
              At "@"
            Error
              Id "x"
            Lit
              StringLit "\"foo\""
            ParenClose ")"
          Error
            Id "b"
          ParenClose ")"
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (a (#eq? @x "foo") b)
      |     ^^^^ tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a (#eq? @x "foo") b)
      |          ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a (#eq? @x "foo") b)
      |           ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a (#eq? @x "foo") b)
      |                    ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "##);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r##"
    Root
      Def
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Error
          Predicate "#match?"
        Error
          At "@"
      Def
        Error
          Id "name"
      Def
        Lit
          StringLit "\"test\""
    ---
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (identifier) #match? @name "test"
      |              ^^^^^^^ tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (identifier) #match? @name "test"
      |                       ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = (identifier)`
      |
    1 | (identifier) #match? @name "test"
      | ^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier)`
    error: unnamed definition must be last in file; add a name: `Name = name`
      |
    1 | (identifier) #match? @name "test"
      |                       ^^^^ unnamed definition must be last in file; add a name: `Name = name`
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

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Error
            Garbage "^^^"
          Error
            Id "b"
          ParenClose ")"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    2 | ^^^
      | ^^^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    3 | b)
      | ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "#);
}

#[test]
fn capture_with_invalid_char() {
    let input = indoc! {r#"
    (identifier) @123
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          At "@"
      Def
        Error
          Garbage "123"
        Error
    ---
    error: expected capture name after '@'
      |
    1 | (identifier) @123
      |               ^^^ expected capture name after '@'
    "#);
}

#[test]
fn field_value_is_garbage() {
    let input = indoc! {r#"
    (call name: %%%)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "call"
          Field
            Id "name"
            Colon ":"
            Error
              Garbage "%%%"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression
      |
    1 | (call name: %%%)
      |             ^^^ unexpected token; expected an expression
    "#);
}

#[test]
fn alternation_recovery_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Error
            Garbage "^^^"
          Error
            At "@"
          Branch
            Error
              Id "name"
          BracketClose "]"
    ---
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [^^^ @name]
      |  ^^^ unexpected token; expected a child expression or closing delimiter
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [^^^ @name]
      |      ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | [^^^ @name]
      |       ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "#);
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "a"
          ParenClose ")"
      Def
        Error
          Garbage "^^^"
      Def
        Id "Expr2"
        Equals "="
        Tree
          ParenOpen "("
          Id "b"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | Expr = (a) ^^^ Expr2 = (b)
      |            ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
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

    insta::assert_snapshot!(query.snapshot_cst(), @r#"
    Root
      Def
        Id "A"
        Equals "="
        Tree
          ParenOpen "("
          Id "a"
          ParenClose ")"
      Def
        Error
          Garbage "^^^"
      Def
        Id "B"
        Equals "="
        Tree
          ParenOpen "("
          Id "b"
          ParenClose ")"
      Def
        Error
          Garbage "$$$"
      Def
        Id "C"
        Equals "="
        Tree
          ParenOpen "("
          Id "c"
          ParenClose ")"
    ---
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    2 | ^^^
      | ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    4 | $$$
      | ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}
