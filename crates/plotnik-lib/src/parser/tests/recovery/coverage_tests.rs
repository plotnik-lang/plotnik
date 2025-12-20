use crate::Query;
use crate::query::query::QueryBuilder;
use indoc::indoc;

#[test]
fn deeply_nested_trees_hit_recursion_limit() {
    let depth = 128;
    let mut input = String::new();
    for _ in 0..depth + 1 {
        input.push_str("(a ");
    }
    for _ in 0..depth {
        input.push(')');
    }

    let result = QueryBuilder::new(&input)
        .with_query_parse_recursion_limit(depth)
        .parse();

    assert!(
        matches!(result, Err(crate::Error::RecursionLimitExceeded)),
        "expected RecursionLimitExceeded error, got {:?}",
        result
    );
}

#[test]
fn deeply_nested_sequences_hit_recursion_limit() {
    let depth = 128;
    let mut input = String::new();
    for _ in 0..depth + 1 {
        input.push_str("{(a) ");
    }
    for _ in 0..depth {
        input.push('}');
    }

    let result = QueryBuilder::new(&input)
        .with_query_parse_recursion_limit(depth)
        .parse();

    assert!(
        matches!(result, Err(crate::Error::RecursionLimitExceeded)),
        "expected RecursionLimitExceeded error, got {:?}",
        result
    );
}

#[test]
fn deeply_nested_alternations_hit_recursion_limit() {
    let depth = 128;
    let mut input = String::new();
    for _ in 0..depth + 1 {
        input.push_str("[(a) ");
    }
    for _ in 0..depth {
        input.push(']');
    }

    let result = QueryBuilder::new(&input)
        .with_query_parse_recursion_limit(depth)
        .parse();

    assert!(
        matches!(result, Err(crate::Error::RecursionLimitExceeded)),
        "expected RecursionLimitExceeded error, got {:?}",
        result
    );
}

#[test]
fn many_trees_exhaust_exec_fuel() {
    let count = 500;
    let mut input = String::new();
    for _ in 0..count {
        input.push_str("(a) ");
    }

    let result = QueryBuilder::new(&input).with_query_parse_fuel(100).parse();

    assert!(
        matches!(result, Err(crate::Error::ExecFuelExhausted)),
        "expected ExecFuelExhausted error, got {:?}",
        result
    );
}

#[test]
fn many_branches_exhaust_exec_fuel() {
    let count = 500;
    let mut input = String::new();
    input.push('[');
    for i in 0..count {
        if i > 0 {
            input.push(' ');
        }
        input.push_str("(a)");
    }
    input.push(']');

    let result = QueryBuilder::new(&input).with_query_parse_fuel(100).parse();

    assert!(
        matches!(result, Err(crate::Error::ExecFuelExhausted)),
        "expected ExecFuelExhausted error, got {:?}",
        result
    );
}

#[test]
fn many_fields_exhaust_exec_fuel() {
    let count = 500;
    let mut input = String::new();
    input.push('(');
    for i in 0..count {
        if i > 0 {
            input.push(' ');
        }
        input.push_str("a: (b)");
    }
    input.push(')');

    let result = QueryBuilder::new(&input).with_query_parse_fuel(100).parse();

    assert!(
        matches!(result, Err(crate::Error::ExecFuelExhausted)),
        "expected ExecFuelExhausted error, got {:?}",
        result
    );
}

#[test]
fn named_def_missing_equals_with_garbage() {
    let input = indoc! {r#"
    Q = Expr ^^^ (identifier)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Q = Expr ^^^ (identifier)
      |     ^^^^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | Q = Expr ^^^ (identifier)
      |          ^^^
    "#);
}

#[test]
fn named_def_missing_equals_recovers_to_next_def() {
    let input = indoc! {r#"
    Broken ^^^
    Valid = (ok)
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r#"
    error: bare identifier is not a valid expression; wrap in parentheses: `(identifier)`
      |
    1 | Broken ^^^
      | ^^^^^^

    error: unexpected token; try `(node)`, `[a b]`, `{a b}`, `"literal"`, or `_`
      |
    1 | Broken ^^^
      |        ^^^
    "#);
}

#[test]
fn empty_double_quote_string() {
    let input = indoc! {r#"
    Q = (a "")
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    Q = (a '')
    "#};

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
fn single_quote_string_is_valid() {
    let input = "Q = (node 'if')";

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    let input = "Q = ['public' 'private']";

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
    let input = r"Q = (node 'it\'s')";

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
fn missing_with_nested_tree_parses() {
    let input = "Q = (MISSING (something))";

    let query = Query::try_from(input).unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_cst(), @r#"
    Root
      Def
        Id "Q"
        Equals "="
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
