use super::helpers_test::*;
use indoc::indoc;

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
        Dot "."
        LowerIdent "bar"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar
      |              ^^^^^^^^
      help: captures become struct fields; use @foo_bar instead
      suggestion: `@foo_bar`
    "#);
}

#[test]
fn capture_dotted_multiple_parts() {
    let input = indoc! {r#"
    (identifier) @foo.bar.baz
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
        Dot "."
        LowerIdent "bar"
        Dot "."
        LowerIdent "baz"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar.baz
      |              ^^^^^^^^^^^^
      help: captures become struct fields; use @foo_bar_baz instead
      suggestion: `@foo_bar_baz`
    "#);
}

#[test]
fn capture_with_space_before_dot_is_valid() {
    let input = indoc! {r#"
    (identifier) @foo . (other)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
      Anchor
        Dot "."
      Node
        ParenOpen "("
        LowerIdent "other"
        ParenClose ")"
    "#);
}

#[test]
fn capture_dotted_followed_by_field() {
    let input = indoc! {r#"
    (node) @foo.bar name: (other)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
        Dot "."
        LowerIdent "bar"
      Field
        LowerIdent "name"
        Colon ":"
        Node
          ParenOpen "("
          LowerIdent "other"
          ParenClose ")"
    ---
    error: capture names cannot contain dots
      |
    1 | (node) @foo.bar name: (other)
      |        ^^^^^^^^
      help: captures become struct fields; use @foo_bar instead
      suggestion: `@foo_bar`
    "#);
}

#[test]
fn capture_space_after_dot_breaks_chain() {
    let input = indoc! {r#"
    (identifier) @foo. bar
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "foo"
        Dot "."
      Node
        LowerIdent "bar"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo. bar
      |              ^^^^^
      help: captures become struct fields; use @foo instead
      suggestion: `@foo`
    "#);
}

#[test]
fn capture() {
    let input = indoc! {r#"
    (identifier) @name
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
    "#);
}

#[test]
fn capture_nested() {
    let input = indoc! {r#"
    (call function: (identifier) @func)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "call"
        Field
          LowerIdent "function"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "func"
        ParenClose ")"
    "#);
}

#[test]
fn multiple_captures() {
    let input = indoc! {r#"
    (binary
        left: (_) @left
        right: (_) @right) @expr
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "binary"
        Field
          LowerIdent "left"
          Colon ":"
          Node
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "left"
        Field
          LowerIdent "right"
          Colon ":"
          Node
            ParenOpen "("
            Underscore "_"
            ParenClose ")"
        Capture
          At "@"
          LowerIdent "right"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "expr"
    "#);
}
