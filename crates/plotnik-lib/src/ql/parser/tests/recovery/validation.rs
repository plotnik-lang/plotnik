use crate::ql::parser::tests::helpers::*;
use indoc::indoc;

// ============================================================================
// Dotted Capture Names
// ============================================================================

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
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
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
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
fn capture_dotted_followed_by_field() {
    let input = indoc! {r#"
    (node) @foo.bar name: (other)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "node"
            ParenClose ")"
          At "@"
          LowerIdent "foo"
          Dot "."
          LowerIdent "bar"
      Def
        Field
          LowerIdent "name"
          Colon ":"
          Tree
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
    error: unnamed definition must be last in file; add a name: `Name = (node) @foo.bar`
      |
    1 | (node) @foo.bar name: (other)
      | ^^^^^^^^^^^^^^^
    "#);
}

#[test]
fn capture_space_after_dot_breaks_chain() {
    let input = indoc! {r#"
    (identifier) @foo. bar
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
          LowerIdent "foo"
          Dot "."
      Def
        Tree
          LowerIdent "bar"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo. bar
      |              ^^^^^
      help: captures become struct fields; use @foo instead
      suggestion: `@foo`
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @foo.`
      |
    1 | (identifier) @foo. bar
      | ^^^^^^^^^^^^^^^^^^
    "#);
}

// ============================================================================
// Single-Quoted Strings
// ============================================================================

#[test]
fn single_quote_string_suggests_double_quotes() {
    let input = "(node 'if')";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Lit
            SingleQuoteLit "'if'"
          ParenClose ")"
    ---
    error: single quotes are not valid for string literals
      |
    1 | (node 'if')
      |       ^^^^
      help: use double quotes
      suggestion: `"if"`
    "#);
}

#[test]
fn single_quote_in_alternation() {
    let input = "['public' 'private']";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Lit
            SingleQuoteLit "'public'"
          Lit
            SingleQuoteLit "'private'"
          BracketClose "]"
    ---
    error: single quotes are not valid for string literals
      |
    1 | ['public' 'private']
      |  ^^^^^^^^
      help: use double quotes
      suggestion: `"public"`
    error: single quotes are not valid for string literals
      |
    1 | ['public' 'private']
      |           ^^^^^^^^^
      help: use double quotes
      suggestion: `"private"`
    "#);
}

#[test]
fn single_quote_with_escape() {
    let input = r"(node 'it\'s')";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Lit
            SingleQuoteLit "'it\\'s'"
          ParenClose ")"
    ---
    error: single quotes are not valid for string literals
      |
    1 | (node 'it\'s')
      |       ^^^^^^^
      help: use double quotes
      suggestion: `"it\'s"`
    "#);
}

// ============================================================================
// Invalid Separators
// ============================================================================

#[test]
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Tree
            ParenOpen "("
            LowerIdent "a"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          ParenClose ")"
    ---
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | (node (a), (b))
      |          ^
      help: remove separator
      suggestion: ``
    "#);
}

#[test]
fn comma_in_alternation() {
    let input = "[a, b, c]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Tree
            LowerIdent "a"
          Tree
            LowerIdent "b"
          Tree
            LowerIdent "c"
          BracketClose "]"
    ---
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [a, b, c]
      |   ^
      help: remove separator
      suggestion: ``
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [a, b, c]
      |      ^
      help: remove separator
      suggestion: ``
    "#);
}

#[test]
fn pipe_in_alternation() {
    let input = "[a | b | c]";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Tree
            LowerIdent "a"
          Tree
            LowerIdent "b"
          Tree
            LowerIdent "c"
          BracketClose "]"
    ---
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [a | b | c]
      |    ^
      help: remove separator
      suggestion: ``
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [a | b | c]
      |        ^
      help: remove separator
      suggestion: ``
    "#);
}

#[test]
fn comma_in_sequence() {
    let input = "{(a), (b)}";

    insta::assert_snapshot!(snapshot(input), @r#"
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
    ---
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | {(a), (b)}
      |     ^
      help: remove separator
      suggestion: ``
    "#);
}

// ============================================================================
// Single Colon for Type Annotation
// ============================================================================

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          At "@"
          LowerIdent "name"
          Type
            Colon ":"
            UpperIdent "Type"
    ---
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name : Type
      |                    ^
      help: use '::'
      suggestion: `::`
    "#);
}

#[test]
fn single_colon_type_annotation_no_space() {
    let input = "(identifier) @name:Type";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          At "@"
          LowerIdent "name"
          Type
            Colon ":"
            UpperIdent "Type"
    ---
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name:Type
      |                   ^
      help: use '::'
      suggestion: `::`
    "#);
}

#[test]
fn single_colon_primitive_type() {
    let input = "@val : string";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Error
          At "@"
        Error
          LowerIdent "val"
          Colon ":"
          LowerIdent "string"
    ---
    error: capture '@' must follow an expression to capture
      |
    1 | @val : string
      | ^
    "#);
}

// ============================================================================
// Lowercase Branch Labels
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
      Def
        Alt
          BracketOpen "["
          Branch
            LowerIdent "left"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
          Branch
            LowerIdent "right"
            Colon ":"
            Tree
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
      Def
        Alt
          BracketOpen "["
          Branch
            LowerIdent "foo"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
          Branch
            UpperIdent "Bar"
            Colon ":"
            Tree
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
// Field Equals Typo
// ============================================================================

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Field
            LowerIdent "name"
            Equals "="
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
          ParenClose ")"
    ---
    error: '=' is not valid for field constraints
      |
    1 | (node name = (identifier))
      |            ^
      help: use ':'
      suggestion: `:`
    "#);
}

#[test]
fn field_equals_typo_no_space() {
    let input = "(node name=(identifier))";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Field
            LowerIdent "name"
            Equals "="
            Tree
              ParenOpen "("
              LowerIdent "identifier"
              ParenClose ")"
          ParenClose ")"
    ---
    error: '=' is not valid for field constraints
      |
    1 | (node name=(identifier))
      |           ^
      help: use ':'
      suggestion: `:`
    "#);
}

// ============================================================================
// Combined Errors
// ============================================================================

#[test]
fn multiple_suggestions_combined() {
    let input = "(node name = 'foo', @val : Type)";

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Field
            LowerIdent "name"
            Equals "="
            Lit
              SingleQuoteLit "'foo'"
          Error
            At "@"
          Field
            LowerIdent "val"
            Error
              Colon ":"
          Tree
            UpperIdent "Type"
          ParenClose ")"
    ---
    error: '=' is not valid for field constraints
      |
    1 | (node name = 'foo', @val : Type)
      |            ^
      help: use ':'
      suggestion: `:`
    error: single quotes are not valid for string literals
      |
    1 | (node name = 'foo', @val : Type)
      |              ^^^^^
      help: use double quotes
      suggestion: `"foo"`
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | (node name = 'foo', @val : Type)
      |                   ^
      help: remove separator
      suggestion: ``
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (node name = 'foo', @val : Type)
      |                     ^
    error: expected ':' to separate field name from its value
      |
    1 | (node name = 'foo', @val : Type)
      |                         ^
    error: unexpected token; expected an expression
      |
    1 | (node name = 'foo', @val : Type)
      |                          ^
    "#);
}

// ============================================================================
// Correct Syntax (No False Positives)
// ============================================================================

#[test]
fn double_quotes_no_error() {
    let input = r#"(node "if")"#;

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
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
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
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
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Field
            LowerIdent "name"
            Colon ":"
            Tree
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
      Def
        Alt
          BracketOpen "["
          Branch
            UpperIdent "Left"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "a"
              ParenClose ")"
          Branch
            UpperIdent "Right"
            Colon ":"
            Tree
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
      Def
        Alt
          BracketOpen "["
          Tree
            LowerIdent "a"
          Tree
            LowerIdent "b"
          Tree
            LowerIdent "c"
          BracketClose "]"
    "#);
}

// ============================================================================
// Resilience Tests (Parser Accepts for Better Error Recovery)
// ============================================================================

#[test]
fn field_with_upper_ident_parses() {
    let input = indoc! {r#"
    (node FieldTypo: (x))
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "node"
          Field
            UpperIdent "FieldTypo"
            Colon ":"
            Tree
              ParenOpen "("
              LowerIdent "x"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn capture_with_upper_ident_parses() {
    let input = indoc! {r#"
    (identifier) @Name
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
          UpperIdent "Name"
    "#);
}

#[test]
fn negated_field_with_upper_ident_parses() {
    let input = indoc! {r#"
    (call !Arguments)
    "#};

    insta::assert_snapshot!(snapshot(input), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          LowerIdent "call"
          NegatedField
            Negation "!"
            UpperIdent "Arguments"
          ParenClose ")"
    "#);
}

#[test]
fn capture_with_type_and_upper_ident() {
    let input = indoc! {r#"
    (identifier) @Name :: MyType
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
          UpperIdent "Name"
          Type
            DoubleColon "::"
            UpperIdent "MyType"
    "#);
}
