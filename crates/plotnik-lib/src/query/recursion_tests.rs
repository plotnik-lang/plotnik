use crate::Query;
use indoc::indoc;

#[test]
fn escape_via_alternation() {
    let query = Query::try_from("E = [(x) (call (E))]").unwrap();

    assert!(query.is_valid());
}

#[test]
fn escape_via_optional() {
    let query = Query::try_from("E = (call (E)?)").unwrap();

    assert!(query.is_valid());
}

#[test]
fn escape_via_star() {
    let query = Query::try_from("E = (call (E)*)").unwrap();

    assert!(query.is_valid());
}

#[test]
fn no_escape_via_plus() {
    let query = Query::try_from("E = (call (E)+)").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = (call (E)+)
      |            ^
      |            |
      |            `E` references itself
    ");
}

#[test]
fn escape_via_empty_tree() {
    let query = Query::try_from("E = [(call) (E)]").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = [(call) (E)]
      |              ^
      |              |
      |              `E` references itself
    ");
}

#[test]
fn lazy_quantifiers_same_as_greedy() {
    assert!(Query::try_from("E = (call (E)??)").unwrap().is_valid());
    assert!(Query::try_from("E = (call (E)*?)").unwrap().is_valid());
    assert!(!Query::try_from("E = (call (E)+?)").unwrap().is_valid());
}

#[test]
fn recursion_in_tree_child() {
    let query = Query::try_from("E = (call (E))").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = (call (E))
      |            ^
      |            |
      |            `E` references itself
    ");
}

#[test]
fn recursion_in_field() {
    let query = Query::try_from("E = (call body: (E))").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = (call body: (E))
      |                  ^
      |                  |
      |                  `E` references itself
    ");
}

#[test]
fn recursion_in_capture() {
    let query = Query::try_from("E = (call (E) @inner)").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = (call (E) @inner)
      |            ^
      |            |
      |            `E` references itself
    ");
}

#[test]
fn recursion_in_sequence() {
    let query = Query::try_from("E = (call {(a) (E)})").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = (call {(a) (E)})
      |                 ^
      |                 |
      |                 `E` references itself
    ");
}

#[test]
fn recursion_through_multiple_children() {
    let query = Query::try_from("E = [(x) (call (a) (E))]").unwrap();

    assert!(query.is_valid());
}

#[test]
fn mutual_recursion_no_escape() {
    let input = indoc! {r#"
        A = (foo (B))
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo (B))
      |           - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           `B` references `A`
    ");
}

#[test]
fn mutual_recursion_one_has_escape() {
    let input = indoc! {r#"
        A = [(x) (foo (B))]
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn three_way_cycle_no_escape() {
    let input = indoc! {r#"
        A = (a (B))
        B = (b (C))
        C = (c (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `C` → `B` → `A` → `C` has no escape path
      |
    1 | A = (a (B))
      |         - `A` references `B`
    2 | B = (b (C))
      |         - `B` references `C` (completing cycle)
    3 | C = (c (A))
      |         ^
      |         |
      |         `C` references `A`
    ");
}

#[test]
fn three_way_cycle_one_has_escape() {
    let input = indoc! {r#"
        A = [(x) (a (B))]
        B = (b (C))
        C = (c (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn diamond_dependency() {
    let input = indoc! {r#"
        A = (a [(B) (C)])
        B = (b (D))
        C = (c (D))
        D = (d (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `C` → `D` → `B` → `A` → `C` has no escape path
      |
    1 | A = (a [(B) (C)])
      |              - `A` references `C` (completing cycle)
    2 | B = (b (D))
    3 | C = (c (D))
      |         ^
      |         |
      |         `C` references `D`
    4 | D = (d (A))
      |         - `D` references `A`
    ");
}

#[test]
fn cycle_ref_in_field() {
    let input = indoc! {r#"
        A = (foo body: (B))
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo body: (B))
      |                 - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           `B` references `A`
    ");
}

#[test]
fn cycle_ref_in_capture() {
    let input = indoc! {r#"
        A = (foo (B) @cap)
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo (B) @cap)
      |           - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           `B` references `A`
    ");
}

#[test]
fn cycle_ref_in_sequence() {
    let input = indoc! {r#"
        A = (foo {(x) (B)})
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo {(x) (B)})
      |                - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           `B` references `A`
    ");
}

#[test]
fn cycle_with_quantifier_escape() {
    let input = indoc! {r#"
        A = (foo (B)?)
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn cycle_with_plus_no_escape() {
    let input = indoc! {r#"
        A = (foo (B)+)
        B = (bar (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo (B)+)
      |           - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           `B` references `A`
    ");
}

#[test]
fn non_recursive_reference() {
    let input = indoc! {r#"
        Leaf = (identifier)
        Tree = (call (Leaf))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn entry_point_uses_recursive_def() {
    let input = indoc! {r#"
        E = [(x) (call (E))]
        (program (E))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn direct_self_ref_in_alternation() {
    // Left-recursion: E calls E without consuming anything.
    // Has escape path (x), but recursive path is unguarded.
    let query = Query::try_from("E = [(E) (x)]").unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `E` → `E` has no escape path
      |
    1 | E = [(E) (x)]
      |       ^
      |       |
      |       `E` references itself
    ");
}

#[test]
fn escape_via_literal_string() {
    // Left-recursion: A calls A without consuming.
    let input = indoc! {r#"
        A = [(A) 'escape']
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = [(A) 'escape']
      |       ^
      |       |
      |       `A` references itself
    ");
}

#[test]
fn escape_via_wildcard() {
    // Left-recursion
    let input = indoc! {r#"
        A = [(A) _]
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = [(A) _]
      |       ^
      |       |
      |       `A` references itself
    ");
}

#[test]
fn escape_via_childless_tree() {
    // Left-recursion
    let input = indoc! {r#"
        A = [(A) (leaf)]
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = [(A) (leaf)]
      |       ^
      |       |
      |       `A` references itself
    ");
}

#[test]
fn escape_via_anchor() {
    let input = indoc! {r#"
        A = (foo . [(A) (x)])
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn no_escape_tree_all_recursive() {
    let input = indoc! {r#"
        A = (foo (A))
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = (foo (A))
      |           ^
      |           |
      |           `A` references itself
    ");
}

#[test]
fn escape_in_capture_inner() {
    let input = indoc! {r#"
        A = [(x)@cap (foo (A))]
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(query.is_valid());
}

#[test]
fn ref_in_quantifier_plus_no_escape() {
    let input = indoc! {r#"
        A = (foo (A)+)
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());
}

#[test]
fn unguarded_recursion_simple() {
    let input = indoc! {r#"
        A = [(A) (foo)]
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = [(A) (foo)]
      |       ^
      |       |
      |       `A` references itself
    ");
}

#[test]
fn unguarded_mutual_recursion() {
    let input = indoc! {r#"
        A = [(B) (x)]
        B = (A)
    "#};
    let query = Query::try_from(input).unwrap();

    assert!(!query.is_valid());

    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: infinite recursion: cycle `A` → `A` has no escape path
      |
    1 | A = [(B) (x)]
      | ^
    ");
}
