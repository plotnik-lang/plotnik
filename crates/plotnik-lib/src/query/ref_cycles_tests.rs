use crate::Query;
use indoc::indoc;

#[test]
fn escape_via_alternation() {
    let query = Query::new("E = [(x) (call (E))]").unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_optional() {
    let query = Query::new("E = (call (E)?)").unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_star() {
    let query = Query::new("E = (call (E)*)").unwrap();
    assert!(query.is_valid());
}

#[test]
fn no_escape_via_plus() {
    let query = Query::new("E = (call (E)+)").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `E` → `E` has no escape path
      |
    1 | E = (call (E)+)
      |            ^
      |            |
      |            recursive pattern can never match: cycle `E` → `E` has no escape path
      |            `E` references itself
    ");
}

#[test]
fn escape_via_empty_tree() {
    let query = Query::new("E = [(call) (E)]").unwrap();
    assert!(query.is_valid());
}

#[test]
fn lazy_quantifiers_same_as_greedy() {
    assert!(Query::new("E = (call (E)??)").unwrap().is_valid());
    assert!(Query::new("E = (call (E)*?)").unwrap().is_valid());
    assert!(!Query::new("E = (call (E)+?)").unwrap().is_valid());
}

#[test]
fn recursion_in_tree_child() {
    let query = Query::new("E = (call (E))").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `E` → `E` has no escape path
      |
    1 | E = (call (E))
      |            ^
      |            |
      |            recursive pattern can never match: cycle `E` → `E` has no escape path
      |            `E` references itself
    ");
}

#[test]
fn recursion_in_field() {
    let query = Query::new("E = (call body: (E))").unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn recursion_in_capture() {
    let query = Query::new("E = (call (E) @inner)").unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn recursion_in_sequence() {
    let query = Query::new("E = (call {(a) (E)})").unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn recursion_through_multiple_children() {
    let query = Query::new("E = [(x) (call (a) (E))]").unwrap();
    assert!(query.is_valid());
}

#[test]
fn mutual_recursion_no_escape() {
    let input = indoc! {r#"
        A = (foo (B))
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo (B))
      |           - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |           `B` references `A`
    ");
}

#[test]
fn mutual_recursion_one_has_escape() {
    let input = indoc! {r#"
        A = [(x) (foo (B))]
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn three_way_cycle_no_escape() {
    let input = indoc! {r#"
        A = (a (B))
        B = (b (C))
        C = (c (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn three_way_cycle_one_has_escape() {
    let input = indoc! {r#"
        A = [(x) (a (B))]
        B = (b (C))
        C = (c (A))
    "#};
    let query = Query::new(input).unwrap();
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
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn cycle_ref_in_field() {
    let input = indoc! {r#"
        A = (foo body: (B))
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo body: (B))
      |                 - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |           `B` references `A`
    ");
}

#[test]
fn cycle_ref_in_capture() {
    let input = indoc! {r#"
        A = (foo (B) @cap)
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo (B) @cap)
      |           - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |           `B` references `A`
    ");
}

#[test]
fn cycle_ref_in_sequence() {
    let input = indoc! {r#"
        A = (foo {(x) (B)})
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @r"
    error: recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |
    1 | A = (foo {(x) (B)})
      |                - `A` references `B` (completing cycle)
    2 | B = (bar (A))
      |           ^
      |           |
      |           recursive pattern can never match: cycle `B` → `A` → `B` has no escape path
      |           `B` references `A`
    ");
}

#[test]
fn cycle_with_quantifier_escape() {
    let input = indoc! {r#"
        A = (foo (B)?)
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn cycle_with_plus_no_escape() {
    let input = indoc! {r#"
        A = (foo (B)+)
        B = (bar (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn non_recursive_reference() {
    let input = indoc! {r#"
        Leaf = (identifier)
        Tree = (call (Leaf))
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn entry_point_uses_recursive_def() {
    let input = indoc! {r#"
        E = [(x) (call (E))]
        (program (E))
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn direct_self_ref_in_alternation() {
    let query = Query::new("E = [(E) (x)]").unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_literal_string() {
    let input = indoc! {r#"
        A = [(A) "escape"]
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_wildcard() {
    let input = indoc! {r#"
        A = [(A) _]
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_childless_tree() {
    let input = indoc! {r#"
        A = [(A) (leaf)]
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn escape_via_anchor() {
    let input = indoc! {r#"
        A = (foo . [(A) (x)])
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn no_escape_tree_all_recursive() {
    let input = indoc! {r#"
        A = (foo (A))
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    assert!(query.dump_errors().contains("recursive pattern"));
}

#[test]
fn escape_in_capture_inner() {
    let input = indoc! {r#"
        A = [(x)@cap (foo (A))]
    "#};
    let query = Query::new(input).unwrap();
    assert!(query.is_valid());
}

#[test]
fn ref_in_quantifier_plus_no_escape() {
    let input = indoc! {r#"
        A = (foo (A)+)
    "#};
    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
}
