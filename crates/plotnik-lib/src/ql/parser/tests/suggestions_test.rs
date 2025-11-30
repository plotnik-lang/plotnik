use super::helpers_test::*;
use indoc::indoc;

// ============================================================================
// Single-quoted strings (should use double quotes)
// ============================================================================

#[test]
fn single_quote_string_suggests_double_quotes() {
    let input = "(node 'if')";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Lit
          SingleQuoteLit "'if'"
        ParenClose ")"
    ---
    error: use double quotes for string literals
      |
    1 | (node 'if')
      |       ^^^^
      help: use double quotes for literals
      suggestion: `"if"`
    "#);
}

#[test]
fn single_quote_in_alternation() {
    let input = "['public' 'private']";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Lit
          SingleQuoteLit "'public'"
        Lit
          SingleQuoteLit "'private'"
        BracketClose "]"
    ---
    error: use double quotes for string literals
      |
    1 | ['public' 'private']
      |  ^^^^^^^^
      help: use double quotes for literals
      suggestion: `"public"`
    error: use double quotes for string literals
      |
    1 | ['public' 'private']
      |           ^^^^^^^^^
      help: use double quotes for literals
      suggestion: `"private"`
    "#);
}

#[test]
fn single_quote_with_escape() {
    let input = r"(node 'it\'s')";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Lit
          SingleQuoteLit "'it\\'s'"
        ParenClose ")"
    ---
    error: use double quotes for string literals
      |
    1 | (node 'it\'s')
      |       ^^^^^^^
      help: use double quotes for literals
      suggestion: `"it\'s"`
    "#);
}

// ============================================================================
// Invalid separators (comma, pipe)
// ============================================================================

#[test]
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        ParenClose ")"
    ---
    error: plotnik uses whitespace for separation; remove ','
      |
    1 | (node (a), (b))
      |          ^
      help: remove ','
      suggestion: ``
    "#);
}

#[test]
fn comma_in_alternation() {
    let input = "[a, b, c]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          LowerIdent "a"
        Node
          LowerIdent "b"
        Node
          LowerIdent "c"
        BracketClose "]"
    ---
    error: plotnik uses whitespace for separation; remove ','
      |
    1 | [a, b, c]
      |   ^
      help: remove ','
      suggestion: ``
    error: plotnik uses whitespace for separation; remove ','
      |
    1 | [a, b, c]
      |      ^
      help: remove ','
      suggestion: ``
    "#);
}

#[test]
fn pipe_in_alternation() {
    let input = "[a | b | c]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          LowerIdent "a"
        Node
          LowerIdent "b"
        Node
          LowerIdent "c"
        BracketClose "]"
    ---
    error: plotnik uses whitespace for separation; remove '|'
      |
    1 | [a | b | c]
      |    ^
      help: remove '|'
      suggestion: ``
    error: plotnik uses whitespace for separation; remove '|'
      |
    1 | [a | b | c]
      |        ^
      help: remove '|'
      suggestion: ``
    "#);
}

#[test]
fn comma_in_sequence() {
    let input = "{(a), (b)}";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Seq
        BraceOpen "{"
        Node
          ParenOpen "("
          LowerIdent "a"
          ParenClose ")"
        Node
          ParenOpen "("
          LowerIdent "b"
          ParenClose ")"
        BraceClose "}"
    ---
    error: plotnik uses whitespace for separation; remove ','
      |
    1 | {(a), (b)}
      |     ^
      help: remove ','
      suggestion: ``
    "#);
}

// ============================================================================
// Single colon for type annotation (should use ::)
// ============================================================================

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        Type
          Colon ":"
          UpperIdent "Type"
    ---
    error: use '::' for type annotations, not ':'
      |
    1 | (identifier) @name : Type
      |                    ^
      help: use '::' for type annotations
      suggestion: `::`
    "#);
}

#[test]
fn single_colon_type_annotation_no_space() {
    let input = "(identifier) @name:Type";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        Type
          Colon ":"
          UpperIdent "Type"
    ---
    error: use '::' for type annotations, not ':'
      |
    1 | (identifier) @name:Type
      |                   ^
      help: use '::' for type annotations
      suggestion: `::`
    "#);
}

#[test]
fn single_colon_primitive_type() {
    let input = "@val : string";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Capture
        At "@"
        LowerIdent "val"
        Type
          Colon ":"
          LowerIdent "string"
    ---
    error: use '::' for type annotations, not ':'
      |
    1 | @val : string
      |      ^
      help: use '::' for type annotations
      suggestion: `::`
    "#);
}

// ============================================================================
// Lowercase branch labels (should be Capitalized)
// ============================================================================

#[test]
fn lowercase_branch_label() {
    let input = indoc! {r#"
    [
      left: (a)
      right: (b)
    ]
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          LowerIdent "left"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
        Branch
          LowerIdent "right"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
        BracketClose "]"
    ---
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    2 |   left: (a)
      |   ^^^^
      help: capitalize as `Left`
      suggestion: `Left`
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    3 |   right: (b)
      |   ^^^^^
      help: capitalize as `Right`
      suggestion: `Right`
    "#);
}

#[test]
fn mixed_case_branch_labels() {
    let input = "[foo: (a) Bar: (b)]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          LowerIdent "foo"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
        Branch
          UpperIdent "Bar"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
        BracketClose "]"
    ---
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    1 | [foo: (a) Bar: (b)]
      |  ^^^
      help: capitalize as `Foo`
      suggestion: `Foo`
    "#);
}

// ============================================================================
// Field equals typo (field = pattern instead of field: pattern)
// ============================================================================

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Field
          LowerIdent "name"
          Equals "="
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    ---
    error: use ':' for field constraints, not '='
      |
    1 | (node name = (identifier))
      |            ^
      help: use ':' for fields
      suggestion: `:`
    "#);
}

#[test]
fn field_equals_typo_no_space() {
    let input = "(node name=(identifier))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Field
          LowerIdent "name"
          Equals "="
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    ---
    error: use ':' for field constraints, not '='
      |
    1 | (node name=(identifier))
      |           ^
      help: use ':' for fields
      suggestion: `:`
    "#);
}

// ============================================================================
// Combined errors (multiple suggestions in one query)
// ============================================================================

#[test]
fn multiple_suggestions_combined() {
    let input = "(node name = 'foo', @val : Type)";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Field
          LowerIdent "name"
          Equals "="
          Lit
            SingleQuoteLit "'foo'"
        Capture
          At "@"
          LowerIdent "val"
          Type
            Colon ":"
            UpperIdent "Type"
        ParenClose ")"
    ---
    error: use ':' for field constraints, not '='
      |
    1 | (node name = 'foo', @val : Type)
      |            ^
      help: use ':' for fields
      suggestion: `:`
    error: use double quotes for string literals
      |
    1 | (node name = 'foo', @val : Type)
      |              ^^^^^
      help: use double quotes for literals
      suggestion: `"foo"`
    error: plotnik uses whitespace for separation; remove ','
      |
    1 | (node name = 'foo', @val : Type)
      |                   ^
      help: remove ','
      suggestion: ``
    error: use '::' for type annotations, not ':'
      |
    1 | (node name = 'foo', @val : Type)
      |                          ^
      help: use '::' for type annotations
      suggestion: `::`
    "#);
}

// ============================================================================
// Correct syntax still works (no false positives)
// ============================================================================

#[test]
fn double_quotes_no_error() {
    let input = r#"(node "if")"#;

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Lit
          StringLit "\"if\""
        ParenClose ")"
    "#);
}

#[test]
fn double_colon_no_error() {
    let input = "(identifier) @name :: Type";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "identifier"
        ParenClose ")"
      Capture
        At "@"
        LowerIdent "name"
        Type
          DoubleColon "::"
          UpperIdent "Type"
    "#);
}

#[test]
fn field_colon_no_error() {
    let input = "(node name: (identifier))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Node
        ParenOpen "("
        LowerIdent "node"
        Field
          LowerIdent "name"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
        ParenClose ")"
    "#);
}

#[test]
fn capitalized_branch_label_no_error() {
    let input = "[Left: (a) Right: (b)]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Branch
          UpperIdent "Left"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
        Branch
          UpperIdent "Right"
          Colon ":"
          Node
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
        BracketClose "]"
    "#);
}

#[test]
fn whitespace_separation_no_error() {
    let input = "[a b c]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Alt
        BracketOpen "["
        Node
          LowerIdent "a"
        Node
          LowerIdent "b"
        Node
          LowerIdent "c"
        BracketClose "]"
    "#);
}
