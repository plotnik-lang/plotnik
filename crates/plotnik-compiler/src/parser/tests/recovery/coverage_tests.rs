use crate::query::QueryBuilder;

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

    let result = QueryBuilder::one_liner(&input)
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

    let result = QueryBuilder::one_liner(&input)
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

    let result = QueryBuilder::one_liner(&input)
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

    let result = QueryBuilder::one_liner(&input)
        .with_query_parse_fuel(100)
        .parse();

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

    let result = QueryBuilder::one_liner(&input)
        .with_query_parse_fuel(100)
        .parse();

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

    let result = QueryBuilder::one_liner(&input)
        .with_query_parse_fuel(100)
        .parse();

    assert!(
        matches!(result, Err(crate::Error::ExecFuelExhausted)),
        "expected ExecFuelExhausted error, got {:?}",
        result
    );
}
