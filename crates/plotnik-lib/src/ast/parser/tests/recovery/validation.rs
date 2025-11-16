use crate::Query;
use indoc::indoc;

#[test]
fn ref_with_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr (child))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr (child))
      |       ^^^^^^^ reference `Expr` cannot contain children
    "#);
}

#[test]
fn ref_with_multiple_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr (a) (b) @cap)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr (a) (b) @cap)
      |       ^^^^^^^^^^^^ reference `Expr` cannot contain children
    "#);
}

#[test]
fn ref_with_field_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr name: (identifier))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr name: (identifier))
      |       ^^^^^^^^^^^^^^^^^^ reference `Expr` cannot contain children
    "#);
}

#[test]
fn ref_without_children_is_valid() {
    let input = indoc! {r#"
    Expr = (identifier)
    (program (Expr) @e)
    "#};

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Expr"
        Equals "="
        Tree
          ParenOpen "("
          Id "identifier"
          ParenClose ")"
      Def
        Tree
          ParenOpen "("
          Id "program"
          Capture
            Ref
              ParenOpen "("
              Id "Expr"
              ParenClose ")"
            At "@"
            Id "e"
          ParenClose ")"
    "#);
}

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar
      |               ^^^^^^^ capture names cannot contain dots
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar.baz
      |               ^^^^^^^^^^^ capture names cannot contain dots
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain dots
      |
    1 | (node) @foo.bar name: (other)
      |         ^^^^^^^ capture names cannot contain dots
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo. bar
      |               ^^^^ capture names cannot contain dots
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

#[test]
fn capture_hyphenated_error() {
    let input = indoc! {r#"
    (identifier) @foo-bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain hyphens
      |
    1 | (identifier) @foo-bar
      |               ^^^^^^^ capture names cannot contain hyphens
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain hyphens
      |
    1 | (identifier) @foo-bar-baz
      |               ^^^^^^^^^^^ capture names cannot contain hyphens
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names cannot contain dots
      |
    1 | (identifier) @foo.bar-baz
      |               ^^^^^^^^^^^ capture names cannot contain dots
      |
    help: captures become struct fields; use @foo_bar_baz instead
      |
    1 - (identifier) @foo.bar-baz
    1 + (identifier) @foo_bar_baz
      |
    "#);
}

#[test]
fn single_quote_string_is_valid() {
    let input = "(node 'if')";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Str
            SingleQuote "'"
            StrVal "if"
            SingleQuote "'"
          ParenClose ")"
    "#);
}

#[test]
fn single_quote_in_alternation() {
    let input = "['public' 'private']";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Str
              SingleQuote "'"
              StrVal "public"
              SingleQuote "'"
          Branch
            Str
              SingleQuote "'"
              StrVal "private"
              SingleQuote "'"
          BracketClose "]"
    "#);
}

#[test]
fn single_quote_with_escape() {
    let input = r"(node 'it\'s')";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Str
            SingleQuote "'"
            StrVal "it\\'s"
            SingleQuote "'"
          ParenClose ")"
    "#);
}

#[test]
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: capture '@' must follow an expression to capture
      |
    1 | @val : string
      | ^ capture '@' must follow an expression to capture
    error: expected ':' to separate field name from its value
      |
    1 | @val : string
      |     ^ expected ':' to separate field name from its value
    error: expected expression after field name
      |
    1 | @val : string
      |      ^ expected expression after field name
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | @val : string
      |        ^^^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = val`
      |
    1 | @val : string
      |  ^^^ unnamed definition must be last in file; add a name: `Name = val`
    ");
}

#[test]
fn lowercase_branch_label() {
    let input = indoc! {r#"
    [
      left: (a)
      right: (b)
    ]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
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

#[test]
fn field_equals_typo_no_expression() {
    let input = "(call name=)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: '=' is not valid for field constraints
      |
    1 | (call name=)
      |           ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (call name=)
    1 + (call name:)
      |
    error: expected expression after field name
      |
    1 | (call name=)
      |            ^ expected expression after field name
    ");
}

#[test]
fn multiple_suggestions_combined() {
    let input = "(node name = 'foo', @val : Type)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
      |                     ^ unexpected token; expected a child expression or closing delimiter
    error: expected ':' to separate field name from its value
      |
    1 | (node name = 'foo', @val : Type)
      |                         ^ expected ':' to separate field name from its value
    error: expected expression after field name
      |
    1 | (node name = 'foo', @val : Type)
      |                          ^ expected expression after field name
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (node name = 'foo', @val : Type)
      |                            ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn double_quotes_no_error() {
    let input = r#"(node "if")"#;

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Str
            DoubleQuote "\""
            StrVal "if"
            DoubleQuote "\""
          ParenClose ")"
    "#);
}

#[test]
fn double_colon_no_error() {
    let input = "(identifier) @name :: Type";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Capture
          Tree
            ParenOpen "("
            Id "identifier"
            ParenClose ")"
          At "@"
          Id "name"
          Type
            DoubleColon "::"
            Id "Type"
    "#);
}

#[test]
fn field_colon_no_error() {
    let input = "(node name: (identifier))";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "node"
          Field
            Id "name"
            Colon ":"
            Tree
              ParenOpen "("
              Id "identifier"
              ParenClose ")"
          ParenClose ")"
    "#);
}

#[test]
fn capitalized_branch_label_no_error() {
    let input = "[Left: (a) Right: (b)]";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Id "Left"
            Colon ":"
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Branch
            Id "Right"
            Colon ":"
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn whitespace_separation_no_error() {
    let input = "[(a) (b) (c)]";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Alt
          BracketOpen "["
          Branch
            Tree
              ParenOpen "("
              Id "a"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "b"
              ParenClose ")"
          Branch
            Tree
              ParenOpen "("
              Id "c"
              ParenClose ")"
          BracketClose "]"
    "#);
}

#[test]
fn field_with_upper_ident_parses() {
    let input = indoc! {r#"
    (node FieldTypo: (x))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: field names must start with lowercase
      |
    1 | (node FieldTypo: (x))
      |       ^^^^^^^^^ field names must start with lowercase
      |
    help: field names must be snake_case; use field_typo: instead
      |
    1 - (node FieldTypo: (x))
    1 + (node field_typo:: (x))
      |
    "#);
}

#[test]
fn capture_with_upper_ident_parses() {
    let input = indoc! {r#"
    (identifier) @Name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names must start with lowercase
      |
    1 | (identifier) @Name
      |               ^^^^ capture names must start with lowercase
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

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: field names must start with lowercase
      |
    1 | (call !Arguments)
      |        ^^^^^^^^^ field names must start with lowercase
      |
    help: field names must be snake_case; use arguments: instead
      |
    1 - (call !Arguments)
    1 + (call !arguments:)
      |
    "#);
}

#[test]
fn capture_with_type_and_upper_ident() {
    let input = indoc! {r#"
    (identifier) @Name :: MyType
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: capture names must start with lowercase
      |
    1 | (identifier) @Name :: MyType
      |               ^^^^ capture names must start with lowercase
      |
    help: capture names must be snake_case; use @name instead
      |
    1 - (identifier) @Name :: MyType
    1 + (identifier) @name :: MyType
      |
    "#);
}

#[test]
fn def_name_lowercase_error() {
    let input = "lowercase = (x)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: definition names must start with uppercase
      |
    1 | lowercase = (x)
      | ^^^^^^^^^ definition names must start with uppercase
      |
    help: definition names must be PascalCase; use Lowercase instead
      |
    1 - lowercase = (x)
    1 + Lowercase = (x)
      |
    ");
}

#[test]
fn def_name_with_underscores_error() {
    let input = "Some_Thing = (x)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: definition names cannot contain separators
      |
    1 | Some_Thing = (x)
      | ^^^^^^^^^^ definition names cannot contain separators
      |
    help: definition names must be PascalCase; use SomeThing instead
      |
    1 - Some_Thing = (x)
    1 + SomeThing = (x)
      |
    ");
}

#[test]
fn def_name_with_hyphens_error() {
    let input = "Some-Thing = (x)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: definition names cannot contain separators
      |
    1 | Some-Thing = (x)
      | ^^^^^^^^^^ definition names cannot contain separators
      |
    help: definition names must be PascalCase; use SomeThing instead
      |
    1 - Some-Thing = (x)
    1 + SomeThing = (x)
      |
    ");
}

#[test]
fn branch_label_with_underscores_error() {
    let input = "[Some_Label: (x)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: branch labels cannot contain separators
      |
    1 | [Some_Label: (x)]
      |  ^^^^^^^^^^ branch labels cannot contain separators
      |
    help: branch labels must be PascalCase; use SomeLabel: instead
      |
    1 - [Some_Label: (x)]
    1 + [SomeLabel:: (x)]
      |
    ");
}

#[test]
fn branch_label_with_hyphens_error() {
    let input = "[Some-Label: (x)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: branch labels cannot contain separators
      |
    1 | [Some-Label: (x)]
      |  ^^^^^^^^^^ branch labels cannot contain separators
      |
    help: branch labels must be PascalCase; use SomeLabel: instead
      |
    1 - [Some-Label: (x)]
    1 + [SomeLabel:: (x)]
      |
    ");
}

#[test]
fn lowercase_branch_label_missing_expression() {
    let input = "[label:]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    1 | [label:]
      |  ^^^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `Label`
      |
    1 - [label:]
    1 + [Label:]
      |
    error: expected expression after branch label
      |
    1 | [label:]
      |        ^ expected expression after branch label
    ");
}

#[test]
fn field_name_with_dots_error() {
    let input = "(call foo.bar: (x))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: field names cannot contain dots
      |
    1 | (call foo.bar: (x))
      |       ^^^^^^^ field names cannot contain dots
      |
    help: field names must be snake_case; use foo_bar: instead
      |
    1 - (call foo.bar: (x))
    1 + (call foo_bar:: (x))
      |
    ");
}

#[test]
fn field_name_with_hyphens_error() {
    let input = "(call foo-bar: (x))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: field names cannot contain hyphens
      |
    1 | (call foo-bar: (x))
      |       ^^^^^^^ field names cannot contain hyphens
      |
    help: field names must be snake_case; use foo_bar: instead
      |
    1 - (call foo-bar: (x))
    1 + (call foo_bar:: (x))
      |
    ");
}

#[test]
fn type_name_with_dots_error() {
    let input = "(x) @name :: Some.Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: type names cannot contain dots or hyphens
      |
    1 | (x) @name :: Some.Type
      |              ^^^^^^^^^ type names cannot contain dots or hyphens
      |
    help: type names cannot contain separators; use ::SomeType instead
      |
    1 - (x) @name :: Some.Type
    1 + (x) @name :: ::SomeType
      |
    ");
}

#[test]
fn type_name_with_hyphens_error() {
    let input = "(x) @name :: Some-Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: type names cannot contain dots or hyphens
      |
    1 | (x) @name :: Some-Type
      |              ^^^^^^^^^ type names cannot contain dots or hyphens
      |
    help: type names cannot contain separators; use ::SomeType instead
      |
    1 - (x) @name :: Some-Type
    1 + (x) @name :: ::SomeType
      |
    ");
}

#[test]
fn unclosed_double_quote_string() {
    let input = r#"(call "foo)"#;

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call "foo)
      |       ^^^^^ unexpected token; expected a child expression or closing delimiter
    error: unclosed tree; expected ')'
      |
    1 | (call "foo)
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    "#);
}

#[test]
fn unclosed_single_quote_string() {
    let input = "(call 'foo)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (call 'foo)
      |       ^^^^^ unexpected token; expected a child expression or closing delimiter
    error: unclosed tree; expected ')'
      |
    1 | (call 'foo)
      | -          ^ unclosed tree; expected ')'
      | |
      | tree started here
    ");
}

#[test]
fn reference_with_supertype_syntax_error() {
    let input = "(RefName/subtype)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: references cannot use supertype syntax (/)
      |
    1 | (RefName/subtype)
      |         ^ references cannot use supertype syntax (/)
    ");
}

#[test]
fn missing_with_nested_tree_parses() {
    let input = "(MISSING (something))";

    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          Tree
            ParenOpen "("
            Id "something"
            ParenClose ")"
          ParenClose ")"
    "#);
}
