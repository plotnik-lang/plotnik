use crate::Query;
use indoc::indoc;

#[test]
fn unexpected_token() {
    let input = indoc! {r#"
    (identifier) ^^^ (string)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | [(a) ^^^ (b)]
      |      ^^^ unexpected token; expected a child expression or closing delimiter
    ");
}

#[test]
fn garbage_inside_node() {
    let input = indoc! {r#"
    (a (b) @@@ (c)) (d)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn xml_tag_garbage() {
    let input = indoc! {r#"
    <div>(identifier)</div>
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
    "#);
}

#[test]
fn predicate_match() {
    let input = indoc! {r#"
    (identifier) #match? @name "test"
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
    "#);
}

#[test]
fn predicate_in_tree() {
    let input = "(function #eq? @name \"test\")";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
fn predicate_in_alternation() {
    let input = indoc! {r#"
    [(a) #eq? (b)]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
      |
    1 | {(a) #set! (b)}
      |      ^^^^^ tree-sitter predicates (#eq?, #match?, #set!, etc.) are not supported
    ");
}

#[test]
fn multiline_garbage_recovery() {
    let input = indoc! {r#"
    (a
    ^^^
    b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    2 | ^^^
      | ^^^ unexpected token; expected a child expression or closing delimiter
    error: bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
      |
    3 | b)
      | ^ bare identifier not allowed; nodes must be enclosed in parentheses, e.g., (identifier)
    ");
}

#[test]
fn top_level_garbage_recovery() {
    let input = indoc! {r#"
    Expr = (a) ^^^ Expr2 = (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

#[test]
fn alternation_recovery_to_capture() {
    let input = indoc! {r#"
    [^^^ @name]
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
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
    ");
}

#[test]
fn comma_between_defs() {
    let input = indoc! {r#"
    A = (a), B = (b)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | A = (a), B = (b)
      |        ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}

#[test]
fn bare_colon_in_tree() {
    let input = "(a : (b))";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: unexpected token; expected a child expression or closing delimiter
      |
    1 | (a : (b))
      |    ^ unexpected token; expected a child expression or closing delimiter
    ");
}

#[test]
fn paren_close_inside_alternation() {
    let input = "[(a) ) (b)]";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
fn bracket_close_inside_sequence() {
    let input = "{(a) ] (b)}";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
    let input = "{(a) ) (b)}";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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

#[test]
fn single_colon_type_annotation_followed_by_non_id() {
    let input = "(a) @x : (b)";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
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
    let input = "(a) @x :";

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
      |
    1 | (a) @x :
      |        ^ unexpected token; expected an expression like (node), [choice], {sequence}, "literal", or _
    "#);
}
