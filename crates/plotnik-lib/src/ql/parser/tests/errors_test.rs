use super::helpers_test::*;
use crate::ql::parser::parse;
use indoc::indoc;

#[test]
fn error_missing_paren() {
    let input = indoc! {r#"
    (identifier
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (identifier
      |            ^
    "#);
}

#[test]
fn error_unexpected_token() {
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
        UnexpectedFragment "^^^"
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
fn error_missing_bracket() {
    let input = indoc! {r#"
    [(identifier) (string)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "string"
          ParenClose ")"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | [(identifier) (string)
      |                       ^
    "#);
}

#[test]
fn error_empty_parens() {
    let input = indoc! {r#"
    ()
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        ParenClose ")"
    ---
    error: empty node pattern - expected node type or children
      |
    1 | ()
      |  ^
    "#);
}

#[test]
fn error_recovery_continues_parsing() {
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
fn error_missing_capture_name() {
    let input = indoc! {r#"
    (identifier) @
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (identifier) @
      |               ^
    "#);
}

#[test]
fn error_missing_field_value() {
    let input = indoc! {r#"
    (call name:)
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
            ParenClose ")"
    ---
    error: unexpected token; expected a pattern
      |
    1 | (call name:)
      |            ^
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (call name:)
      |             ^
    "#);
}

#[test]
fn error_unexpected_xml_tag() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        UnexpectedXML "<div>"
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Error
        UnexpectedXML "</div>"
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
fn error_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (a (b (c)
      |          ^
    "#);
}

#[test]
fn error_multiple_consecutive() {
    let input = indoc! {r#"
    ^^^ $$$ %%% (ok)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        UnexpectedFragment "^^^"
      Error
        UnexpectedFragment "$$$"
      Error
        UnexpectedFragment "%%%"
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
fn error_inside_alternation() {
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
          UnexpectedFragment "^^^"
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
fn error_empty_alternation() {
    let input = indoc! {r#"
    []
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        BracketClose "]"
    "#);
}

#[test]
fn error_unclosed_alternation_nested() {
    let input = indoc! {r#"
    [(a) (b
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | [(a) (b
      |        ^
    "#);
}

#[test]
fn error_at_start() {
    let input = indoc! {r#"
    ^^^ (a)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        UnexpectedFragment "^^^"
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
fn error_only_garbage() {
    let input = indoc! {r#"
    ^^^ $$$
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        UnexpectedFragment "^^^"
      Error
        UnexpectedFragment "$$$"
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
fn error_negated_field_missing_name() {
    let input = indoc! {r#"
    (call !)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        NegatedField
          Negation "!"
        ParenClose ")"
    ---
    error: expected field name after '!' (e.g., !value)
      |
    1 | (call !)
      |        ^
    "#);
}

#[test]
fn error_field_missing_colon() {
    let input = indoc! {r#"
    (call name (identifier))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Node
          LowerIdent "name"
        Node
          ParenOpen "("
          LowerIdent "identifier"
          ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn error_capture_with_invalid_char() {
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
        UnexpectedFragment "123"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (identifier) @123
      |               ^^^
    "#);
}

#[test]
fn error_deeply_nested_unclosed() {
    let input = indoc! {r#"
    (a (b (c (d
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        Node
          ParenOpen "("
          LowerIdent "b"
          Node
            ParenOpen "("
            LowerIdent "c"
            Node
              ParenOpen "("
              LowerIdent "d"
    ---
    error: unexpected end of input inside node; expected a child pattern or closing delimiter
      |
    1 | (a (b (c (d
      |            ^
    "#);
}

#[test]
fn error_mixed_valid_invalid_captures() {
    let input = indoc! {r#"
    (a) @ok @ @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "a"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "ok"
      Capture
        At "@"
      Capture
        At "@"
        LowerIdent "name"
    ---
    error: expected capture name after '@' (e.g., @name, @my_var)
      |
    1 | (a) @ok @ @name
      |           ^
    "#);
}

#[test]
fn error_xml_self_closing() {
    let input = indoc! {r#"
    <br/> (a)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Error
        UnexpectedXML "<br/>"
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
fn error_recovery_alternation_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Error
          UnexpectedFragment "^^^"
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
fn error_multiline_recovery() {
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
          UnexpectedFragment "^^^"
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
fn error_field_value_is_error_token() {
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
            UnexpectedFragment "%%%"
        ParenClose ")"
    ---
    error: unexpected token; expected a pattern
      |
    1 | (call name: %%%)
      |             ^^^
    "#);
}

#[test]
fn error_json_serialization() {
    let input = "(identifier) @foo.bar";
    let result = parse(input);
    let errors = result.errors();

    assert_eq!(errors.len(), 1);
    let json = serde_json::to_string_pretty(&errors[0]).unwrap();

    insta::assert_snapshot!(json, @r#"
    {
      "range": {
        "start": 13,
        "end": 21
      },
      "message": "capture names cannot contain dots",
      "fix": {
        "replacement": "@foo_bar",
        "description": "captures become struct fields; use @foo_bar instead"
      }
    }
    "#);
}

#[test]
fn error_json_serialization_no_fix() {
    let input = "(identifier) @";
    let result = parse(input);
    let errors = result.errors();

    assert_eq!(errors.len(), 1);
    let json = serde_json::to_string_pretty(&errors[0]).unwrap();

    insta::assert_snapshot!(json, @r#"
    {
      "range": {
        "start": 14,
        "end": 14
      },
      "message": "expected capture name after '@' (e.g., @name, @my_var)"
    }
    "#);
}
