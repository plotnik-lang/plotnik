use crate::Query;
use indoc::indoc;

// ============================================================================
// Dotted Capture Names
// ============================================================================

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
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
          CaptureName "@foo.bar"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar
      |              ^^^^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_bar instead
      |
    1 - (identifier) @foo.bar
    1 + (identifier) @foo_bar
      |
    "#);
}

#[test]
fn capture_dotted_multiple_parts() {
    let input = indoc! {r#"
    (identifier) @foo.bar.baz
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
          CaptureName "@foo.bar.baz"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar.baz
      |              ^^^^^^^^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_bar_baz instead
      |
    1 - (identifier) @foo.bar.baz
    1 + (identifier) @foo_bar_baz
      |
    "#);
}

#[test]
fn capture_dotted_followed_by_field() {
    let input = indoc! {r#"
    (node) @foo.bar name: (other)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "node"
            ParenClose ")"
          CaptureName "@foo.bar"
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
      |        ^^^^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_bar instead
      |
    1 - (node) @foo.bar name: (other)
    1 + (node) @foo_bar name: (other)
      |
    error: unnamed definition must be last in file; add a name: `Name = (node) @foo.bar`
      |
    1 | (node) @foo.bar name: (other)
      | ^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (node) @foo.bar`
    "#);
}

#[test]
fn capture_space_after_dot_breaks_chain() {
    let input = indoc! {r#"
    (identifier) @foo. bar
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
          CaptureName "@foo."
      Def
        Error
          LowerIdent "bar"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo. bar
      |              ^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_ instead
      |
    1 - (identifier) @foo. bar
    1 + (identifier) @foo_ bar
      |
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (identifier) @foo. bar
      |                    ^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @foo.`
      |
    1 | (identifier) @foo. bar
      | ^^^^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier) @foo.`
    "#);
}

// ============================================================================
// Hyphenated Capture Names
// ============================================================================

#[test]
fn capture_hyphenated_error() {
    let input = indoc! {r#"
    (identifier) @foo-bar
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
          CaptureName "@foo-bar"
    ---
    error: capture names cannot contain hyphens
      |
    1 | (identifier) @foo-bar
      |              ^^^^^^^^ capture names cannot contain hyphens
      |
    help: captures become struct fields; use @foo_bar instead
      |
    1 - (identifier) @foo-bar
    1 + (identifier) @foo_bar
      |
    "#);
}

#[test]
fn capture_hyphenated_multiple() {
    let input = indoc! {r#"
    (identifier) @foo-bar-baz
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
          CaptureName "@foo-bar-baz"
    ---
    error: capture names cannot contain hyphens
      |
    1 | (identifier) @foo-bar-baz
      |              ^^^^^^^^^^^^ capture names cannot contain hyphens
      |
    help: captures become struct fields; use @foo_bar_baz instead
      |
    1 - (identifier) @foo-bar-baz
    1 + (identifier) @foo_bar_baz
      |
    "#);
}

#[test]
fn capture_mixed_dots_and_hyphens() {
    let input = indoc! {r#"
    (identifier) @foo.bar-baz
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
          CaptureName "@foo.bar-baz"
    ---
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar-baz
      |              ^^^^^^^^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_bar_baz instead
      |
    1 - (identifier) @foo.bar-baz
    1 + (identifier) @foo_bar_baz
      |
    "#);
}

// ============================================================================
// Single Quote Strings
// ============================================================================

#[test]
fn single_quote_string_suggests_double_quotes() {
    let input = "(node 'if')";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |       ^^^^ single quotes are not valid for string literals
      |
    help: use double quotes
      |
    1 - (node 'if')
    1 + (node "if")
      |
    "#);
}

#[test]
fn single_quote_in_alternation() {
    let input = "['public' 'private']";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |  ^^^^^^^^ single quotes are not valid for string literals
      |
    help: use double quotes
      |
    1 - ['public' 'private']
    1 + ["public" 'private']
      |
    error: single quotes are not valid for string literals
      |
    1 | ['public' 'private']
      |           ^^^^^^^^^ single quotes are not valid for string literals
      |
    help: use double quotes
      |
    1 - ['public' 'private']
    1 + ['public' "private"]
      |
    "#);
}

#[test]
fn single_quote_with_escape() {
    let input = r"(node 'it\'s')";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |       ^^^^^^^ single quotes are not valid for string literals
      |
    help: use double quotes
      |
    1 - (node 'it\'s')
    1 + (node "it\'s")
      |
    "#);
}

// ============================================================================
// Invalid Separators
// ============================================================================

#[test]
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |          ^ ',' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - (node (a), (b))
    1 + (node (a) (b))
      |
    "#);
}

#[test]
fn comma_in_alternation() {
    let input = "[(a), (b), (c)]";

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
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
          BracketClose "]"
    ---
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [(a), (b), (c)]
      |     ^ ',' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - [(a), (b), (c)]
    1 + [(a) (b), (c)]
      |
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [(a), (b), (c)]
      |          ^ ',' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - [(a), (b), (c)]
    1 + [(a), (b) (c)]
      |
    "#);
}

#[test]
fn pipe_in_alternation() {
    let input = "[(a) | (b) | (c)]";

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
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
          BracketClose "]"
    ---
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [(a) | (b) | (c)]
      |      ^ '|' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - [(a) | (b) | (c)]
    1 + [(a)  (b) | (c)]
      |
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [(a) | (b) | (c)]
      |            ^ '|' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - [(a) | (b) | (c)]
    1 + [(a) | (b)  (c)]
      |
    "#);
}

#[test]
fn comma_in_sequence() {
    let input = "{(a), (b)}";

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
    ---
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | {(a), (b)}
      |     ^ ',' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - {(a), (b)}
    1 + {(a) (b)}
      |
    "#);
}

// ============================================================================
// Single Colon for Type Annotation
// ============================================================================

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
          Type
            Colon ":"
            UpperIdent "Type"
    ---
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name : Type
      |                    ^ single colon is not valid for type annotations
      |
    help: use '::'
      |
    1 | (identifier) @name :: Type
      |                     +
    "#);
}

#[test]
fn single_colon_type_annotation_no_space() {
    let input = "(identifier) @name:Type";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
          Type
            Colon ":"
            UpperIdent "Type"
    ---
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name:Type
      |                   ^ single colon is not valid for type annotations
      |
    help: use '::'
      |
    1 | (identifier) @name::Type
      |                    +
    "#);
}

#[test]
fn single_colon_primitive_type() {
    let input = "@val : string";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Error
          CaptureName "@val"
        Error
          Colon ":"
          LowerIdent "string"
    ---
    error: capture '@' must follow an expression to capture
      |
    1 | @val : string
      | ^^^^ capture '@' must follow an expression to capture
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |   ^^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Left`
      |
    2 -   left: (a)
    2 +   Left: (a)
      |
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    3 |   right: (b)
      |   ^^^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Right`
      |
    3 -   right: (b)
    3 +   Right: (b)
      |
    "#);
}

#[test]
fn mixed_case_branch_labels() {
    let input = "[foo: (a) Bar: (b)]";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |  ^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Foo`
      |
    1 - [foo: (a) Bar: (b)]
    1 + [Foo: (a) Bar: (b)]
      |
    "#);
}

// ============================================================================
// Field Equals Typo
// ============================================================================

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |            ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (node name = (identifier))
    1 + (node name : (identifier))
      |
    "#);
}

#[test]
fn field_equals_typo_no_space() {
    let input = "(node name=(identifier))";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
      |           ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (node name=(identifier))
    1 + (node name:(identifier))
      |
    "#);
}

// ============================================================================
// Combined Errors
// ============================================================================

#[test]
fn multiple_suggestions_combined() {
    let input = "(node name = 'foo', @val : Type)";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
            CaptureName "@val"
          Error
            Colon ":"
          Error
            UpperIdent "Type"
          ParenClose ")"
    ---
    error: '=' is not valid for field constraints
      |
    1 | (node name = 'foo', @val : Type)
      |            ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (node name = 'foo', @val : Type)
    1 + (node name : 'foo', @val : Type)
      |
    error: single quotes are not valid for string literals
      |
    1 | (node name = 'foo', @val : Type)
      |              ^^^^^ single quotes are not valid for string literals
      |
    help: use double quotes
      |
    1 - (node name = 'foo', @val : Type)
    1 + (node name = "foo", @val : Type)
      |
    error: ',' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | (node name = 'foo', @val : Type)
      |                   ^ ',' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - (node name = 'foo', @val : Type)
    1 + (node name = 'foo' @val : Type)
      |
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (node name = 'foo', @val : Type)
      |                     ^^^^ unexpected token; expected a child expression or closing delimiter
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (node name = 'foo', @val : Type)
      |                          ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (node name = 'foo', @val : Type)
      |                            ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "#);
}

// ============================================================================
// Correct Syntax (No False Positives)
// ============================================================================

#[test]
fn double_quotes_no_error() {
    let input = r#"(node "if")"#;

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@name"
          Type
            DoubleColon "::"
            UpperIdent "Type"
    "#);
}

#[test]
fn field_colon_no_error() {
    let input = "(node name: (identifier))";

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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
    let input = "[(a) (b) (c)]";

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
          Tree
            ParenOpen "("
            LowerIdent "b"
            ParenClose ")"
          Tree
            ParenOpen "("
            LowerIdent "c"
            ParenClose ")"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@Name"
    ---
    error: capture names must start with lowercase
      |
    1 | (identifier) @Name
      |              ^^^^^ capture names must start with lowercase
      |
    help: capture names must be snake_case; use @name instead
      |
    1 - (identifier) @Name
    1 + (identifier) @name
      |
    "#);
}

#[test]
fn negated_field_with_upper_ident_parses() {
    let input = indoc! {r#"
    (call !Arguments)
    "#};

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
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

    let query = Query::new(input);

    insta::assert_snapshot!(query.snapshot_ast(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            LowerIdent "identifier"
            ParenClose ")"
          CaptureName "@Name"
          Type
            DoubleColon "::"
            UpperIdent "MyType"
    ---
    error: capture names must start with lowercase
      |
    1 | (identifier) @Name :: MyType
      |              ^^^^^ capture names must start with lowercase
      |
    help: capture names must be snake_case; use @name instead
      |
    1 - (identifier) @Name :: MyType
    1 + (identifier) @name :: MyType
      |
    "#);
}
