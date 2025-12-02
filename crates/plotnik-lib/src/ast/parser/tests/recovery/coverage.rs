//! Additional tests for parser coverage gaps.

use crate::Query;
#[cfg(debug_assertions)]
use crate::ast::ParserOptions;
use indoc::indoc;

#[test]
fn named_def_missing_equals_with_garbage() {
    let input = indoc! {r#"
    Expr ^^^ (identifier)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Expr ^^^ (identifier)
      | ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | Expr ^^^ (identifier)
      |      ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = Expr`
      |
    1 | Expr ^^^ (identifier)
      | ^^^^ unnamed definition must be last in file; add a name: `Name = Expr`
    "#);
}

#[test]
fn named_def_missing_equals_recovers_to_next_def() {
    let input = indoc! {r#"
    Broken ^^^
    Valid = (ok)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | Broken ^^^
      | ^^^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | Broken ^^^
      |        ^^^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn def_name_snake_case_suggests_pascal() {
    let input = indoc! {r#"
    my_expr = (identifier)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn branch_label_snake_case_suggests_pascal() {
    let input = indoc! {r#"
    [My_branch: (a) Other: (b)]
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn type_annotation_dotted_suggests_pascal() {
    let input = indoc! {r#"
    (a) @x :: My.Type
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn lowercase_branch_label_suggests_capitalized() {
    let input = indoc! {r#"
    [first: (a) Second: (b)]
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn predicate_in_alternation() {
    let input = indoc! {r#"
    [(a) #eq? (b)]
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [(a) #eq? (b)]
      |      ^^^^ unexpected token; expected a child expression or closing delimiter
    ");
}

#[test]
fn predicate_in_sequence() {
    let input = indoc! {r#"
    {(a) #set! (b)}
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | {(a) #set! (b)}
      |      ^^^^^ tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
    ");
}

#[test]
fn bare_capture_at_root() {
    let input = indoc! {r#"
    @name
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: capture '@' must follow an expression to capture
      |
    1 | @name
      | ^ capture '@' must follow an expression to capture
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | @name
      |  ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn capture_at_start_of_alternation() {
    let input = indoc! {r#"
    [@x (a)]
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [@x (a)]
      |  ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | [@x (a)]
      |   ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn deeply_nested_trees_hit_recursion_limit() {
    // MAX_DEPTH is 512, so 520 levels should hit the limit
    let depth = 520;
    let mut input = String::new();
    for _ in 0..depth {
        input.push_str("(a ");
    }
    for _ in 0..depth {
        input.push(')');
    }

    #[cfg(debug_assertions)]
    let query = Query::with_options(&input, ParserOptions { disable_fuel: true });
    #[cfg(not(debug_assertions))]
    let query = Query::new(&input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursion limit exceeded
      |
    1 | ... (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a (a ))))))))))))))))))))))))))))))))))))))))))))))))))))))...
      |                                                        ^ recursion limit exceeded
    error: unclosed tree; expected ')'
      |
    1 | ...(a (a (a (a (a (a (a (a (a (a ))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))
      |       - tree started here                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                ^ unclosed tree; expected ')'
    ");
}

#[test]
fn unclosed_tree_shows_open_location() {
    let input = indoc! {r#"
    (call
        (identifier)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unclosed tree; expected ')'
      |
    1 | (call
      | - tree started here
    2 |     (identifier)
      |                 ^ unclosed tree; expected ')'
    ");
}

#[test]
fn unclosed_alternation_shows_open_location() {
    let input = indoc! {r#"
    [
        (a)
        (b)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unclosed alternation; expected ']'
      |
    1 | [
      | - alternation started here
    2 |     (a)
    3 |     (b)
      |        ^ unclosed alternation; expected ']'
    ");
}

#[test]
fn unclosed_sequence_shows_open_location() {
    let input = indoc! {r#"
    {
        (a)
        (b)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unclosed sequence; expected '}'
      |
    1 | {
      | - sequence started here
    2 |     (a)
    3 |     (b)
      |        ^ unclosed sequence; expected '}'
    ");
}

#[test]
fn single_colon_type_annotation_with_space() {
    let input = indoc! {r#"
    (a) @x : Type
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn field_equals_typo_in_tree() {
    let input = indoc! {r#"
    (call name = (identifier))
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn field_equals_typo_missing_value() {
    let input = indoc! {r#"
    (call name = )
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: '=' is not valid for field constraints
      |
    1 | (call name = )
      |            ^ '=' is not valid for field constraints
      |
    help: use ':'
      |
    1 - (call name = )
    1 + (call name : )
      |
    error: expected expression after field name
      |
    1 | (call name = )
      |              ^ expected expression after field name
    ");
}

#[test]
fn comma_between_defs() {
    let input = indoc! {r#"
    A = (a), B = (b)
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | A = (a), B = (b)
      |        ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn pipe_between_branches() {
    let input = indoc! {r#"
    [(a) | (b)]
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn empty_double_quote_string() {
    let input = indoc! {r#"
    (a "")
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Str
            DoubleQuote "\""
            DoubleQuote "\""
          ParenClose ")"
    "#);
}

#[test]
fn empty_single_quote_string() {
    let input = indoc! {r#"
    (a '')
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "a"
          Str
            SingleQuote "'"
            SingleQuote "'"
          ParenClose ")"
    "#);
}

#[test]
fn supertype_with_string_arg() {
    let input = indoc! {r#"
    (expression/binary)
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          Id "expression"
          Slash "/"
          Id "binary"
          ParenClose ")"
    "#);
}

#[test]
fn missing_node_syntax() {
    let input = indoc! {r#"
    (MISSING "identifier")
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwMissing "MISSING"
          DoubleQuote "\""
          StrVal "identifier"
          DoubleQuote "\""
          ParenClose ")"
    "#);
}

#[test]
fn error_node_syntax() {
    let input = indoc! {r#"
    (ERROR)
    "#};

    let query = Query::new(input);
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Tree
          ParenOpen "("
          KwError "ERROR"
          ParenClose ")"
    "#);
}

#[test]
fn capture_name_pascal_case_error() {
    let input = indoc! {r#"
    (a) @Name
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
    // This tests to_snake_case with hyphens via the uppercase branch
    let input = indoc! {r#"
    (a) @My-Name
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn field_name_pascal_case_error() {
    let input = indoc! {r#"
    (call Name: (a))
    "#};

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn bare_capture_at_eof_triggers_sync() {
    // After error_and_bump consumes '@', we're at EOF
    // synchronize_to_def_start should return false (eof branch)
    let input = "@";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: capture '@' must follow an expression to capture
      |
    1 | @
      | ^ capture '@' must follow an expression to capture
    ");
}

#[test]
fn bare_colon_in_tree() {
    let input = "(a : (b))";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a : (b))
      |    ^ unexpected token; expected a child expression or closing delimiter
    ");
}

#[test]
fn pipe_in_tree() {
    let input = "(a | b)";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
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
fn paren_close_inside_alternation() {
    let input = "[(a) ) (b)]";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected closing ']' for alternation
      |
    1 | [(a) ) (b)]
      |      ^ expected closing ']' for alternation
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | [(a) ) (b)]
      |           ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = [(a)`
      |
    1 | [(a) ) (b)]
      | ^^^^ unnamed definition must be last in file; add a name: `Name = [(a)`
    "#);
}

#[test]
fn type_annotation_missing_name() {
    let input = "(a) @x ::";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | (a) @x ::
      |          ^ expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn type_annotation_missing_name_with_bracket() {
    let input = "[(a) @x :: ]";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: expected type name after '::' (e.g., ::MyType or ::string)
      |
    1 | [(a) @x :: ]
      |            ^ expected type name after '::' (e.g., ::MyType or ::string)
    ");
}

#[test]
fn predicate_in_tree() {
    let input = "(function #eq? @name \"test\")";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | (function #eq? @name "test")
      |           ^^^^ tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (function #eq? @name "test")
      |                ^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    1 | (function #eq? @name "test")
      |                 ^^^^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    "#);
}

#[test]
fn single_colon_type_annotation_followed_by_non_id() {
    // @x : followed by ( which is not an Id - triggers early return in parse_type_annotation_single_colon
    let input = "(a) @x : (b)";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (a) @x : (b)
      |        ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = (a) @x`
      |
    1 | (a) @x : (b)
      | ^^^^^^ unnamed definition must be last in file; add a name: `Name = (a) @x`
    "#);
}

#[test]
fn single_colon_type_annotation_at_eof() {
    // @x : at end of input
    let input = "(a) @x :";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (a) @x :
      |        ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn bracket_close_inside_sequence() {
    // ] inside {} triggers SEQ_RECOVERY break
    let input = "{(a) ] (b)}";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected closing '}' for sequence
      |
    1 | {(a) ] (b)}
      |      ^ expected closing '}' for sequence
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | {(a) ] (b)}
      |           ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = {(a)`
      |
    1 | {(a) ] (b)}
      | ^^^^ unnamed definition must be last in file; add a name: `Name = {(a)`
    "#);
}

#[test]
fn paren_close_inside_sequence() {
    // ) inside {} triggers SEQ_RECOVERY break
    let input = "{(a) ) (b)}";

    let query = Query::new(input);
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r#"
    error: expected closing '}' for sequence
      |
    1 | {(a) ) (b)}
      |      ^ expected closing '}' for sequence
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | {(a) ) (b)}
      |           ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    error: unnamed definition must be last in file; add a name: `Name = {(a)`
      |
    1 | {(a) ) (b)}
      | ^^^^ unnamed definition must be last in file; add a name: `Name = {(a)`
    "#);
}
