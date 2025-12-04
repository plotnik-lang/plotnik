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
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr (child))
      |       ^^^^^^^ reference `Expr` cannot contain children
    ");
}

#[test]
fn ref_with_multiple_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr (a) (b) @cap)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr (a) (b) @cap)
      |       ^^^^^^^^^^^^ reference `Expr` cannot contain children
    ");
}

#[test]
fn ref_with_field_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr name: (identifier))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: reference `Expr` cannot contain children
      |
    2 | (Expr name: (identifier))
      |       ^^^^^^^^^^^^^^^^^^ reference `Expr` cannot contain children
    ");
}

#[test]
fn reference_with_supertype_syntax_error() {
    let input = "(RefName/subtype)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: references cannot use supertype syntax (/)
      |
    1 | (RefName/subtype)
      |         ^ references cannot use supertype syntax (/)
    ");
}

#[test]
fn mixed_tagged_and_untagged() {
    let input = indoc! {r#"
    [Tagged: (a) (b) Another: (c)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: mixed tagged and untagged branches in alternation
      |
    1 | [Tagged: (a) (b) Another: (c)]
      |  ------      ^^^ mixed tagged and untagged branches in alternation
      |  |
      |  tagged branch here
    ");
}

#[test]
fn error_with_unexpected_content() {
    let input = indoc! {r#"
    (ERROR (something))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: (ERROR) takes no arguments
      |
    1 | (ERROR (something))
      |        ^ (ERROR) takes no arguments
    ");
}

#[test]
fn bare_error_keyword() {
    let input = indoc! {r#"
    ERROR
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | ERROR
      | ^^^^^ ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
    ");
}

#[test]
fn bare_missing_keyword() {
    let input = indoc! {r#"
    MISSING
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
      |
    1 | MISSING
      | ^^^^^^^ ERROR and MISSING must be inside parentheses: (ERROR) or (MISSING ...)
    ");
}

#[test]
fn upper_ident_in_alternation_not_followed_by_colon() {
    let input = indoc! {r#"
    [(Expr) (Statement)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: undefined reference: `Expr`
      |
    1 | [(Expr) (Statement)]
      |   ^^^^ undefined reference: `Expr`
    error: undefined reference: `Statement`
      |
    1 | [(Expr) (Statement)]
      |          ^^^^^^^^^ undefined reference: `Statement`
    ");
}

#[test]
fn upper_ident_not_followed_by_equals_is_expression() {
    let input = indoc! {r#"
    (Expr)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: undefined reference: `Expr`
      |
    1 | (Expr)
      |  ^^^^ undefined reference: `Expr`
    ");
}

#[test]
fn bare_upper_ident_not_followed_by_equals_is_error() {
    let input = indoc! {r#"
    Expr
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr
      | ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn named_def_missing_equals() {
    let input = indoc! {r#"
    Expr (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr (identifier)
      | ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unnamed definition must be last in file; add a name: `Name = Expr`
      |
    1 | Expr (identifier)
      | ^^^^ unnamed definition must be last in file; add a name: `Name = Expr`
    ");
}

#[test]
fn unnamed_def_not_allowed_in_middle() {
    let input = indoc! {r#"
    (first)
    Expr = (identifier)
    (last)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^ unnamed definition must be last in file; add a name: `Name = (first)`
    ");
}

#[test]
fn multiple_unnamed_defs_errors_for_all_but_last() {
    let input = indoc! {r#"
    (first)
    (second)
    (third)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unnamed definition must be last in file; add a name: `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^ unnamed definition must be last in file; add a name: `Name = (first)`
    error: unnamed definition must be last in file; add a name: `Name = (second)`
      |
    2 | (second)
      | ^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (second)`
    ");
}

#[test]
fn capture_space_after_dot_is_anchor() {
    let input = indoc! {r#"
    (identifier) @foo . (other)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unnamed definition must be last in file; add a name: `Name = (identifier) @foo`
      |
    1 | (identifier) @foo . (other)
      | ^^^^^^^^^^^^^^^^^ unnamed definition must be last in file; add a name: `Name = (identifier) @foo`
    error: unnamed definition must be last in file; add a name: `Name = .`
      |
    1 | (identifier) @foo . (other)
      |                   ^ unnamed definition must be last in file; add a name: `Name = .`
    ");
}

#[test]
fn def_name_lowercase_error() {
    let input = "lowercase = (x)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn def_name_snake_case_suggests_pascal() {
    let input = indoc! {r#"
    my_expr = (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: definition names must start with uppercase
      |
    1 | my_expr = (identifier)
      | ^^^^^^^ definition names must start with uppercase
      |
    help: definition names must be PascalCase; use MyExpr instead
      |
    1 - my_expr = (identifier)
    1 + MyExpr = (identifier)
      |
    ");
}

#[test]
fn def_name_kebab_case_suggests_pascal() {
    let input = indoc! {r#"
    my-expr = (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: definition names must start with uppercase
      |
    1 | my-expr = (identifier)
      | ^^^^^^^ definition names must start with uppercase
      |
    help: definition names must be PascalCase; use MyExpr instead
      |
    1 - my-expr = (identifier)
    1 + MyExpr = (identifier)
      |
    ");
}

#[test]
fn def_name_dotted_suggests_pascal() {
    let input = indoc! {r#"
    my.expr = (identifier)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: definition names must start with uppercase
      |
    1 | my.expr = (identifier)
      | ^^^^^^^ definition names must start with uppercase
      |
    help: definition names must be PascalCase; use MyExpr instead
      |
    1 - my.expr = (identifier)
    1 + MyExpr = (identifier)
      |
    ");
}

#[test]
fn def_name_with_underscores_error() {
    let input = "Some_Thing = (x)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn capture_name_pascal_case_error() {
    let input = indoc! {r#"
    (a) @Name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture names must start with lowercase
      |
    1 | (a) @Name
      |      ^^^^ capture names must start with lowercase
      |
    help: capture names must be snake_case; use @name instead
      |
    1 - (a) @Name
    1 + (a) @name
      |
    ");
}

#[test]
fn capture_name_pascal_case_with_hyphens_error() {
    let input = indoc! {r#"
    (a) @My-Name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture names cannot contain hyphens
      |
    1 | (a) @My-Name
      |      ^^^^^^^ capture names cannot contain hyphens
      |
    help: captures become struct fields; use @my_name instead
      |
    1 - (a) @My-Name
    1 + (a) @my_name
      |
    ");
}

#[test]
fn capture_name_with_hyphens_error() {
    let input = indoc! {r#"
    (a) @my-name
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: capture names cannot contain hyphens
      |
    1 | (a) @my-name
      |      ^^^^^^^ capture names cannot contain hyphens
      |
    help: captures become struct fields; use @my_name instead
      |
    1 - (a) @my-name
    1 + (a) @my_name
      |
    ");
}

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_dotted_multiple_parts() {
    let input = indoc! {r#"
    (identifier) @foo.bar.baz
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_dotted_followed_by_field() {
    let input = indoc! {r#"
    (node) @foo.bar name: (other)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_space_after_dot_breaks_chain() {
    let input = indoc! {r#"
    (identifier) @foo. bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_hyphenated_error() {
    let input = indoc! {r#"
    (identifier) @foo-bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_hyphenated_multiple() {
    let input = indoc! {r#"
    (identifier) @foo-bar-baz
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn capture_mixed_dots_and_hyphens() {
    let input = indoc! {r#"
    (identifier) @foo.bar-baz
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn field_name_pascal_case_error() {
    let input = indoc! {r#"
    (call Name: (a))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: field names must start with lowercase
      |
    1 | (call Name: (a))
      |       ^^^^ field names must start with lowercase
      |
    help: field names must be snake_case; use name: instead
      |
    1 - (call Name: (a))
    1 + (call name:: (a))
      |
    ");
}

#[test]
fn field_name_with_dots_error() {
    let input = "(call foo.bar: (x))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn negated_field_with_upper_ident_parses() {
    let input = indoc! {r#"
    (call !Arguments)
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn branch_label_snake_case_suggests_pascal() {
    let input = indoc! {r#"
    [My_branch: (a) Other: (b)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: branch labels cannot contain separators
      |
    1 | [My_branch: (a) Other: (b)]
      |  ^^^^^^^^^ branch labels cannot contain separators
      |
    help: branch labels must be PascalCase; use MyBranch: instead
      |
    1 - [My_branch: (a) Other: (b)]
    1 + [MyBranch:: (a) Other: (b)]
      |
    ");
}

#[test]
fn branch_label_kebab_case_suggests_pascal() {
    let input = indoc! {r#"
    [My-branch: (a) Other: (b)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: branch labels cannot contain separators
      |
    1 | [My-branch: (a) Other: (b)]
      |  ^^^^^^^^^ branch labels cannot contain separators
      |
    help: branch labels must be PascalCase; use MyBranch: instead
      |
    1 - [My-branch: (a) Other: (b)]
    1 + [MyBranch:: (a) Other: (b)]
      |
    ");
}

#[test]
fn branch_label_dotted_suggests_pascal() {
    let input = indoc! {r#"
    [My.branch: (a) Other: (b)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: branch labels cannot contain separators
      |
    1 | [My.branch: (a) Other: (b)]
      |  ^^^^^^^^^ branch labels cannot contain separators
      |
    help: branch labels must be PascalCase; use MyBranch: instead
      |
    1 - [My.branch: (a) Other: (b)]
    1 + [MyBranch:: (a) Other: (b)]
      |
    ");
}

#[test]
fn branch_label_with_underscores_error() {
    let input = "[Some_Label: (x)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn lowercase_branch_label() {
    let input = indoc! {r#"
    [
      left: (a)
      right: (b)
    ]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn lowercase_branch_label_suggests_capitalized() {
    let input = indoc! {r#"
    [first: (a) Second: (b)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: tagged alternation labels must be Capitalized (they map to enum variants)
      |
    1 | [first: (a) Second: (b)]
      |  ^^^^^ tagged alternation labels must be Capitalized (they map to enum variants)
      |
    help: capitalize as `First`
      |
    1 - [first: (a) Second: (b)]
    1 + [First: (a) Second: (b)]
      |
    ");
}

#[test]
fn mixed_case_branch_labels() {
    let input = "[foo: (a) Bar: (b)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn type_annotation_dotted_suggests_pascal() {
    let input = indoc! {r#"
    (a) @x :: My.Type
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: type names cannot contain dots or hyphens
      |
    1 | (a) @x :: My.Type
      |           ^^^^^^^ type names cannot contain dots or hyphens
      |
    help: type names cannot contain separators; use ::MyType instead
      |
    1 - (a) @x :: My.Type
    1 + (a) @x :: ::MyType
      |
    ");
}

#[test]
fn type_annotation_kebab_suggests_pascal() {
    let input = indoc! {r#"
    (a) @x :: My-Type
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: type names cannot contain dots or hyphens
      |
    1 | (a) @x :: My-Type
      |           ^^^^^^^ type names cannot contain dots or hyphens
      |
    help: type names cannot contain separators; use ::MyType instead
      |
    1 - (a) @x :: My-Type
    1 + (a) @x :: ::MyType
      |
    ");
}

#[test]
fn type_name_with_dots_error() {
    let input = "(x) @name :: Some.Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn comma_in_alternation() {
    let input = "[(a), (b), (c)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn comma_in_sequence() {
    let input = "{(a), (b)}";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn pipe_in_alternation() {
    let input = "[(a) | (b) | (c)]";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn pipe_between_branches() {
    let input = indoc! {r#"
    [(a) | (b)]
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | [(a) | (b)]
      |      ^ '|' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - [(a) | (b)]
    1 + [(a)  (b)]
      |
    ");
}

#[test]
fn pipe_in_tree() {
    let input = "(a | b)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | (a | b)
      |    ^ '|' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - (a | b)
    1 + (a  b)
      |
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (a | b)
      |      ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn pipe_in_sequence() {
    let input = "{(a) | (b)}";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: '|' is not valid syntax; plotnik uses whitespace for separation
      |
    1 | {(a) | (b)}
      |      ^ '|' is not valid syntax; plotnik uses whitespace for separation
      |
    help: remove separator
      |
    1 - {(a) | (b)}
    1 + {(a)  (b)}
      |
    ");
}

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn field_equals_typo_no_space() {
    let input = "(node name=(identifier))";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn field_equals_typo_no_expression() {
    let input = "(call name=)";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
fn field_equals_typo_in_tree() {
    let input = indoc! {r#"
    (call name = (identifier))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: '=' is not valid for field constraints
      |
    1 | (call name = (identifier))
      |            ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (call name = (identifier))
    1 + (call name : (identifier))
      |
    ");
}

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name : Type
      |                    ^ single colon is not valid for type annotations
      |
    help: use '::'
      |
    1 | (identifier) @name :: Type
      |                     +
    ");
}

#[test]
fn single_colon_type_annotation_no_space() {
    let input = "(identifier) @name:Type";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: single colon is not valid for type annotations
      |
    1 | (identifier) @name:Type
      |                   ^ single colon is not valid for type annotations
      |
    help: use '::'
      |
    1 | (identifier) @name::Type
      |                    +
    ");
}

#[test]
fn single_colon_type_annotation_with_space() {
    let input = indoc! {r#"
    (a) @x : Type
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: single colon is not valid for type annotations
      |
    1 | (a) @x : Type
      |        ^ single colon is not valid for type annotations
      |
    help: use '::'
      |
    1 | (a) @x :: Type
      |         +
    ");
}

#[test]
fn single_colon_primitive_type() {
    let input = "@val : string";

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
