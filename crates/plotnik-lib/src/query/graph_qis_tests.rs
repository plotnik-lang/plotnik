use indoc::indoc;

use crate::Query;

fn check_qis(source: &str) -> String {
    let query = Query::try_from(source).unwrap().build_graph();
    let mut result = Vec::new();

    for def in query.root().defs() {
        let def_name = def.name().map(|t| t.text().to_string()).unwrap_or_default();
        let mut triggers: Vec<_> = query
            .qis_triggers
            .iter()
            .filter_map(|(q, trigger)| {
                // Check if this quantifier belongs to this definition
                let q_range = q.text_range();
                let def_range = def.text_range();
                if q_range.start() >= def_range.start() && q_range.end() <= def_range.end() {
                    Some((
                        q_range.start(),
                        format!("  QIS: [{}]", trigger.captures.join(", ")),
                    ))
                } else {
                    None
                }
            })
            .collect();
        triggers.sort_by_key(|(pos, _)| *pos);
        let triggers: Vec<_> = triggers.into_iter().map(|(_, s)| s).collect();

        if triggers.is_empty() {
            result.push(format!("{}: no QIS", def_name));
        } else {
            result.push(format!("{}:", def_name));
            result.extend(triggers);
        }
    }

    result.join("\n")
}

#[test]
fn single_capture_no_qis() {
    let source = "Foo = { (a) @x }*";

    insta::assert_snapshot!(check_qis(source), @"Foo: no QIS");
}

#[test]
fn two_captures_triggers_qis() {
    let source = "Foo = { (a) @x (b) @y }*";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
    ");
}

#[test]
fn three_captures_triggers_qis() {
    let source = "Foo = { (a) @x (b) @y (c) @z }*";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y, z]
    ");
}

#[test]
fn captured_sequence_absorbs_inner() {
    let source = "Foo = { { (a) @x (b) @y } @inner }*";

    insta::assert_snapshot!(check_qis(source), @"Foo: no QIS");
}

#[test]
fn captured_alternation_absorbs_inner() {
    let source = "Foo = { [ (a) @x (b) @y ] @choice }*";

    insta::assert_snapshot!(check_qis(source), @"Foo: no QIS");
}

#[test]
fn uncaptured_alternation_propagates() {
    let source = "Foo = { [ (a) @x (b) @y ] }*";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
    ");
}

#[test]
fn node_with_two_captures() {
    let source = indoc! {r#"
        Foo = (function
            name: (identifier) @name
            body: (block) @body
        )*
    "#};

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [name, body]
    ");
}

#[test]
fn plus_quantifier_triggers_qis() {
    let source = "Foo = { (a) @x (b) @y }+";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
    ");
}

#[test]
fn optional_quantifier_triggers_qis() {
    let source = "Foo = { (a) @x (b) @y }?";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
    ");
}

#[test]
fn nested_quantifier_inner_qis() {
    let source = "Foo = { { (a) @x (b) @y }* }+";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
      QIS: [x, y]
    ");
}

#[test]
fn nested_quantifier_both_qis() {
    // Outer quantifier has @c and @inner (2 captures) -> QIS
    // Inner quantifier has @x and @y (2 captures) -> QIS
    let source = "Outer = { (c) @c { (a) @x (b) @y }* @inner }+";

    insta::assert_snapshot!(check_qis(source), @r"
    Outer:
      QIS: [c, inner]
      QIS: [x, y]
    ");
}

#[test]
fn multiple_definitions() {
    let source = indoc! {r#"
        Single = { (a) @x }*
        Multi = { (a) @x (b) @y }*
    "#};

    insta::assert_snapshot!(check_qis(source), @r"
    Single: no QIS
    Multi:
      QIS: [x, y]
    ");
}

#[test]
fn no_quantifier_no_qis() {
    let source = "Foo = { (a) @x (b) @y }";

    insta::assert_snapshot!(check_qis(source), @"Foo: no QIS");
}

#[test]
fn lazy_quantifier_triggers_qis() {
    let source = "Foo = { (a) @x (b) @y }*?";

    insta::assert_snapshot!(check_qis(source), @r"
    Foo:
      QIS: [x, y]
    ");
}

#[test]
fn qis_graph_has_object_effects() {
    // Verify that QIS-triggered quantifiers emit StartObject/EndObject
    let source = "Foo = { (a) @x (b) @y }*";
    let (_query, pre_opt) = Query::try_from(source)
        .unwrap()
        .build_graph_with_pre_opt_dump();

    // QIS adds StartObj/EndObj around each iteration to keep captures coupled.
    // Multi-capture definitions also get wrapped in StartObj/EndObj at root.
    let start_count = pre_opt.matches("StartObj").count();
    let end_count = pre_opt.matches("EndObj").count();

    // 1 from multi-capture def wrapper + 1 from QIS loop = 2
    assert_eq!(
        start_count, 2,
        "QIS graph should have 2 StartObj (multi-capture def + QIS loop):\n{}",
        pre_opt
    );
    assert_eq!(
        end_count, 2,
        "QIS graph should have 2 EndObj (multi-capture def + QIS loop):\n{}",
        pre_opt
    );
}

#[test]
fn non_qis_graph_no_object_effects() {
    // Single capture should NOT trigger QIS object wrapping
    let source = "Foo = { (a) @x }*";
    let (_query, pre_opt) = Query::try_from(source)
        .unwrap()
        .build_graph_with_pre_opt_dump();

    // Non-QIS quantifiers don't need object scope - captures propagate with array cardinality.
    // Sequences themselves don't add object scope either.
    let start_count = pre_opt.matches("StartObj").count();
    let end_count = pre_opt.matches("EndObj").count();

    assert_eq!(
        start_count, 0,
        "Non-QIS graph should have no StartObj:\n{}",
        pre_opt
    );
    assert_eq!(
        end_count, 0,
        "Non-QIS graph should have no EndObj:\n{}",
        pre_opt
    );
}
