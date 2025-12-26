use crate::Query;
use indoc::indoc;

#[test]
fn ref_with_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr (child))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is a reference and cannot have children
      |
    2 | (Expr (child))
      |       ^^^^^^^
    ");
}

#[test]
fn ref_with_multiple_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr (a) (b) @cap)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is a reference and cannot have children
      |
    2 | (Expr (a) (b) @cap)
      |       ^^^^^^^^^^^^
    ");
}

#[test]
fn ref_with_field_children_error() {
    let input = indoc! {r#"
    Expr = (identifier)
    (Expr name: (identifier))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is a reference and cannot have children
      |
    2 | (Expr name: (identifier))
      |       ^^^^^^^^^^^^^^^^^^
    ");
}

#[test]
fn reference_with_supertype_syntax_error() {
    let input = "(RefName/subtype)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: supertype syntax not allowed on references
      |
    1 | (RefName/subtype)
      |         ^
    ");
}

#[test]
fn mixed_tagged_and_untagged() {
    let input = indoc! {r#"
    [Tagged: (a) (b) Another: (c)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | [Tagged: (a) (b) Another: (c)]
      |  ------      ^^^
      |  |
      |  tagged branch here
    ");
}

#[test]
fn error_with_unexpected_content() {
    let input = indoc! {r#"
    (ERROR (something))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `(ERROR)` cannot have child nodes
      |
    1 | (ERROR (something))
      |        ^
    ");
}

#[test]
fn bare_error_keyword() {
    let input = indoc! {r#"
    ERROR
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `ERROR` and `MISSING` must be wrapped in parentheses
      |
    1 | ERROR
      | ^^^^^
    ");
}

#[test]
fn bare_missing_keyword() {
    let input = indoc! {r#"
    MISSING
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `ERROR` and `MISSING` must be wrapped in parentheses
      |
    1 | MISSING
      | ^^^^^^^
    ");
}

#[test]
fn upper_ident_in_alternation_not_followed_by_colon() {
    let input = indoc! {r#"
    [(Expr) (Statement)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is not defined
      |
    1 | [(Expr) (Statement)]
      |   ^^^^

    error: `Statement` is not defined
      |
    1 | [(Expr) (Statement)]
      |          ^^^^^^^^^
    ");
}

#[test]
fn upper_ident_not_followed_by_equals_is_expression() {
    let input = indoc! {r#"
    (Expr)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `Expr` is not defined
      |
    1 | (Expr)
      |  ^^^^
    ");
}

#[test]
fn bare_upper_ident_not_followed_by_equals_is_error() {
    let input = indoc! {r#"
    Expr
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Expr
      | ^^^^
    ");
}

#[test]
fn named_def_missing_equals() {
    let input = indoc! {r#"
    Expr (identifier)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Expr (identifier)
      | ^^^^
    ");
}

#[test]
fn unnamed_def_not_allowed_in_middle() {
    let input = indoc! {r#"
    (first)
    Expr = (identifier)
    (last)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definitions must be named — give it a name like `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^

    error: definitions must be named — give it a name like `Name = (last)`
      |
    3 | (last)
      | ^^^^^^
    ");
}

#[test]
fn multiple_unnamed_defs_errors_for_all_but_last() {
    let input = indoc! {r#"
    (first)
    (second)
    (third)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definitions must be named — give it a name like `Name = (first)`
      |
    1 | (first)
      | ^^^^^^^

    error: definitions must be named — give it a name like `Name = (second)`
      |
    2 | (second)
      | ^^^^^^^^

    error: definitions must be named — give it a name like `Name = (third)`
      |
    3 | (third)
      | ^^^^^^^
    ");
}

#[test]
fn capture_space_after_dot_is_anchor() {
    let input = indoc! {r#"
    (identifier) @foo . (other)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definitions must be named — give it a name like `Name = (identifier) @foo`
      |
    1 | (identifier) @foo . (other)
      | ^^^^^^^^^^^^^^^^^

    error: definitions must be named — give it a name like `Name = .`
      |
    1 | (identifier) @foo . (other)
      |                   ^

    error: definitions must be named — give it a name like `Name = (other)`
      |
    1 | (identifier) @foo . (other)
      |                     ^^^^^^^
    ");
}

#[test]
fn def_name_lowercase_error() {
    let input = "lowercase = (x)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must start uppercase; definitions map to types
      |
    1 | lowercase = (x)
      | ^^^^^^^^^
      |
    help: use `Lowercase`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must start uppercase; definitions map to types
      |
    1 | my_expr = (identifier)
      | ^^^^^^^
      |
    help: use `MyExpr`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must start uppercase; definitions map to types
      |
    1 | my-expr = (identifier)
      | ^^^^^^^
      |
    help: use `MyExpr`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must start uppercase; definitions map to types
      |
    1 | my.expr = (identifier)
      | ^^^^^^^
      |
    help: use `MyExpr`
      |
    1 - my.expr = (identifier)
    1 + MyExpr = (identifier)
      |
    ");
}

#[test]
fn def_name_with_underscores_error() {
    let input = "Some_Thing = (x)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must be PascalCase; definitions map to types
      |
    1 | Some_Thing = (x)
      | ^^^^^^^^^^
      |
    help: use `SomeThing`
      |
    1 - Some_Thing = (x)
    1 + SomeThing = (x)
      |
    ");
}

#[test]
fn def_name_with_hyphens_error() {
    let input = "Some-Thing = (x)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: definition names must be PascalCase; definitions map to types
      |
    1 | Some-Thing = (x)
      | ^^^^^^^^^^
      |
    help: use `SomeThing`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names must be lowercase; captures become struct fields
      |
    1 | (a) @Name
      |      ^^^^
      |
    help: use `@name`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `-`; captures become struct fields
      |
    1 | (a) @My-Name
      |      ^^^^^^^
      |
    help: use `@my_name`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `-`; captures become struct fields
      |
    1 | (a) @my-name
      |      ^^^^^^^
      |
    help: use `@my_name`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `.`; captures become struct fields
      |
    1 | (identifier) @foo.bar
      |               ^^^^^^^
      |
    help: use `@foo_bar`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `.`; captures become struct fields
      |
    1 | (identifier) @foo.bar.baz
      |               ^^^^^^^^^^^
      |
    help: use `@foo_bar_baz`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `.`; captures become struct fields
      |
    1 | (node) @foo.bar name: (other)
      |         ^^^^^^^
      |
    help: use `@foo_bar`
      |
    1 - (node) @foo.bar name: (other)
    1 + (node) @foo_bar name: (other)
      |
    ");
}

#[test]
fn capture_space_after_dot_breaks_chain() {
    let input = indoc! {r#"
    (identifier) @foo. bar
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `.`; captures become struct fields
      |
    1 | (identifier) @foo. bar
      |               ^^^^
      |
    help: use `@foo_`
      |
    1 - (identifier) @foo. bar
    1 + (identifier) @foo_ bar
      |

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (identifier) @foo. bar
      |                    ^^^
    ");
}

#[test]
fn capture_hyphenated_error() {
    let input = indoc! {r#"
    (identifier) @foo-bar
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `-`; captures become struct fields
      |
    1 | (identifier) @foo-bar
      |               ^^^^^^^
      |
    help: use `@foo_bar`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `-`; captures become struct fields
      |
    1 | (identifier) @foo-bar-baz
      |               ^^^^^^^^^^^
      |
    help: use `@foo_bar_baz`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: capture names cannot contain `.`; captures become struct fields
      |
    1 | (identifier) @foo.bar-baz
      |               ^^^^^^^^^^^
      |
    help: use `@foo_bar_baz`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field names must be lowercase; field names become struct fields
      |
    1 | (call Name: (a))
      |       ^^^^
      |
    help: use `name:`
      |
    1 - (call Name: (a))
    1 + (call name:: (a))
      |
    ");
}

#[test]
fn field_name_with_dots_error() {
    let input = "(call foo.bar: (x))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field names cannot contain `.`; field names become struct fields
      |
    1 | (call foo.bar: (x))
      |       ^^^^^^^
      |
    help: use `foo_bar:`
      |
    1 - (call foo.bar: (x))
    1 + (call foo_bar:: (x))
      |
    ");
}

#[test]
fn field_name_with_hyphens_error() {
    let input = "(call foo-bar: (x))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field names cannot contain `-`; field names become struct fields
      |
    1 | (call foo-bar: (x))
      |       ^^^^^^^
      |
    help: use `foo_bar:`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: field names must be lowercase; field names become struct fields
      |
    1 | (call !Arguments)
      |        ^^^^^^^^^
      |
    help: use `arguments:`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be PascalCase; branch labels map to enum variants
      |
    1 | [My_branch: (a) Other: (b)]
      |  ^^^^^^^^^
      |
    help: use `MyBranch:`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be PascalCase; branch labels map to enum variants
      |
    1 | [My-branch: (a) Other: (b)]
      |  ^^^^^^^^^
      |
    help: use `MyBranch:`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be PascalCase; branch labels map to enum variants
      |
    1 | [My.branch: (a) Other: (b)]
      |  ^^^^^^^^^
      |
    help: use `MyBranch:`
      |
    1 - [My.branch: (a) Other: (b)]
    1 + [MyBranch:: (a) Other: (b)]
      |
    ");
}

#[test]
fn branch_label_with_underscores_error() {
    let input = "[Some_Label: (x)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be PascalCase; branch labels map to enum variants
      |
    1 | [Some_Label: (x)]
      |  ^^^^^^^^^^
      |
    help: use `SomeLabel:`
      |
    1 - [Some_Label: (x)]
    1 + [SomeLabel:: (x)]
      |
    ");
}

#[test]
fn branch_label_with_hyphens_error() {
    let input = "[Some-Label: (x)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be PascalCase; branch labels map to enum variants
      |
    1 | [Some-Label: (x)]
      |  ^^^^^^^^^^
      |
    help: use `SomeLabel:`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be capitalized; branch labels map to enum variants
      |
    2 |   left: (a)
      |   ^^^^
      |
    help: use `Left`
      |
    2 -   left: (a)
    2 +   Left: (a)
      |

    error: branch labels must be capitalized; branch labels map to enum variants
      |
    3 |   right: (b)
      |   ^^^^^
      |
    help: use `Right`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be capitalized; branch labels map to enum variants
      |
    1 | [first: (a) Second: (b)]
      |  ^^^^^
      |
    help: use `First`
      |
    1 - [first: (a) Second: (b)]
    1 + [First: (a) Second: (b)]
      |
    ");
}

#[test]
fn mixed_case_branch_labels() {
    let input = "[foo: (a) Bar: (b)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: branch labels must be capitalized; branch labels map to enum variants
      |
    1 | [foo: (a) Bar: (b)]
      |  ^^^
      |
    help: use `Foo`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type names cannot contain `.` or `-`; type annotations map to types
      |
    1 | (a) @x :: My.Type
      |           ^^^^^^^
      |
    help: use `::MyType`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type names cannot contain `.` or `-`; type annotations map to types
      |
    1 | (a) @x :: My-Type
      |           ^^^^^^^
      |
    help: use `::MyType`
      |
    1 - (a) @x :: My-Type
    1 + (a) @x :: ::MyType
      |
    ");
}

#[test]
fn type_name_with_dots_error() {
    let input = "(x) @name :: Some.Type";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type names cannot contain `.` or `-`; type annotations map to types
      |
    1 | (x) @name :: Some.Type
      |              ^^^^^^^^^
      |
    help: use `::SomeType`
      |
    1 - (x) @name :: Some.Type
    1 + (x) @name :: ::SomeType
      |
    ");
}

#[test]
fn type_name_with_hyphens_error() {
    let input = "(x) @name :: Some-Type";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type names cannot contain `.` or `-`; type annotations map to types
      |
    1 | (x) @name :: Some-Type
      |              ^^^^^^^^^
      |
    help: use `::SomeType`
      |
    1 - (x) @name :: Some-Type
    1 + (x) @name :: ::SomeType
      |
    ");
}

#[test]
fn comma_in_node_children() {
    let input = "(node (a), (b))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `,`
      |
    1 | (node (a), (b))
      |          ^
      |
    help: remove
      |
    1 - (node (a), (b))
    1 + (node (a) (b))
      |
    ");
}

#[test]
fn comma_in_alternation() {
    let input = "[(a), (b), (c)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `,`
      |
    1 | [(a), (b), (c)]
      |     ^
      |
    help: remove
      |
    1 - [(a), (b), (c)]
    1 + [(a) (b), (c)]
      |

    error: separators are not needed; plotnik uses whitespace, not `,`
      |
    1 | [(a), (b), (c)]
      |          ^
      |
    help: remove
      |
    1 - [(a), (b), (c)]
    1 + [(a), (b) (c)]
      |
    ");
}

#[test]
fn comma_in_sequence() {
    let input = "{(a), (b)}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `,`
      |
    1 | {(a), (b)}
      |     ^
      |
    help: remove
      |
    1 - {(a), (b)}
    1 + {(a) (b)}
      |
    ");
}

#[test]
fn pipe_in_alternation() {
    let input = "[(a) | (b) | (c)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `|`
      |
    1 | [(a) | (b) | (c)]
      |      ^
      |
    help: remove
      |
    1 - [(a) | (b) | (c)]
    1 + [(a)  (b) | (c)]
      |

    error: separators are not needed; plotnik uses whitespace, not `|`
      |
    1 | [(a) | (b) | (c)]
      |            ^
      |
    help: remove
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `|`
      |
    1 | [(a) | (b)]
      |      ^
      |
    help: remove
      |
    1 - [(a) | (b)]
    1 + [(a)  (b)]
      |
    ");
}

#[test]
fn pipe_in_tree() {
    let input = "(a | b)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `|`
      |
    1 | (a | b)
      |    ^
      |
    help: remove
      |
    1 - (a | b)
    1 + (a  b)
      |

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | (a | b)
      |      ^
    ");
}

#[test]
fn pipe_in_sequence() {
    let input = "{(a) | (b)}";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: separators are not needed; plotnik uses whitespace, not `|`
      |
    1 | {(a) | (b)}
      |      ^
      |
    help: remove
      |
    1 - {(a) | (b)}
    1 + {(a)  (b)}
      |
    ");
}

#[test]
fn field_equals_typo() {
    let input = "(node name = (identifier))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: use `:` for field constraints, not `=`; this isn't a definition
      |
    1 | (node name = (identifier))
      |            ^
      |
    help: use `:`
      |
    1 - (node name = (identifier))
    1 + (node name : (identifier))
      |
    ");
}

#[test]
fn field_equals_typo_no_space() {
    let input = "(node name=(identifier))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: use `:` for field constraints, not `=`; this isn't a definition
      |
    1 | (node name=(identifier))
      |           ^
      |
    help: use `:`
      |
    1 - (node name=(identifier))
    1 + (node name:(identifier))
      |
    ");
}

#[test]
fn field_equals_typo_no_expression() {
    let input = "(call name=)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: use `:` for field constraints, not `=`; this isn't a definition
      |
    1 | (call name=)
      |           ^
      |
    help: use `:`
      |
    1 - (call name=)
    1 + (call name:)
      |
    ");
}

#[test]
fn field_equals_typo_in_tree() {
    let input = indoc! {r#"
    (call name = (identifier))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: use `:` for field constraints, not `=`; this isn't a definition
      |
    1 | (call name = (identifier))
      |            ^
      |
    help: use `:`
      |
    1 - (call name = (identifier))
    1 + (call name : (identifier))
      |
    ");
}

#[test]
fn single_colon_type_annotation() {
    let input = "(identifier) @name : Type";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type annotations use `::`, not `:` — single `:` looks like a field
      |
    1 | (identifier) @name : Type
      |                    ^
      |
    help: use `::`
      |
    1 | (identifier) @name :: Type
      |                     +
    ");
}

#[test]
fn single_colon_type_annotation_no_space() {
    let input = "(identifier) @name:Type";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type annotations use `::`, not `:` — single `:` looks like a field
      |
    1 | (identifier) @name:Type
      |                   ^
      |
    help: use `::`
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

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: type annotations use `::`, not `:` — single `:` looks like a field
      |
    1 | (a) @x : Type
      |        ^
      |
    help: use `::`
      |
    1 | (a) @x :: Type
      |         +
    ");
}

#[test]
fn single_colon_primitive_type() {
    let input = "@val : string";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: `@` must follow an expression to capture
      |
    1 | @val : string
      | ^

    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | @val : string
      |        ^^^^^^
    ");
}

#[test]
fn treesitter_sequence_syntax_warning() {
    // Tree-sitter uses ((a) (b)) for sequences, Plotnik uses {(a) (b)}
    let input = "Test = ((a) (b))";

    let res = Query::expect_warning(input);

    insta::assert_snapshot!(res, @r"
    warning: Tree-sitter sequence syntax
      |
    1 | Test = ((a) (b))
      |        ^
      |
    help: use `{...}` for sequences
    ");
}

#[test]
fn treesitter_sequence_single_child_warning() {
    let input = "Test = ((a))";

    let res = Query::expect_warning(input);

    insta::assert_snapshot!(res, @r"
    warning: Tree-sitter sequence syntax
      |
    1 | Test = ((a))
      |        ^
      |
    help: use `{...}` for sequences
    ");
}

#[test]
fn named_node_with_children_no_warning() {
    // Normal node with children - NOT a tree-sitter sequence
    Query::expect_valid("Test = (identifier (child))");
}
