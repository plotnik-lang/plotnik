use crate::Query;
use indoc::indoc;

#[test]
fn valid_recursion_with_alternation_base_case() {
    let input = "E = [(x) (call (E))]";
    Query::expect_valid(input);
}

#[test]
fn valid_recursion_with_optional() {
    let input = "E = (call (E)?)";
    Query::expect_valid(input);
}

#[test]
fn valid_recursion_with_star() {
    let input = "E = (call (E)*)";
    Query::expect_valid(input);
}

#[test]
fn invalid_recursion_with_plus() {
    let input = "E = (call (E)+)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | E = (call (E)+)
      |            ^
      |            |
      |            E references itself
    ");
}

#[test]
fn invalid_unguarded_recursion_in_alternation() {
    let input = "E = [(call) (E)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | E = [(call) (E)]
      |              ^
      |              |
      |              references itself
    ");
}

#[test]
fn validity_of_lazy_quantifiers_matches_greedy() {
    Query::expect_valid("E = (call (E)??)");
    Query::expect_valid("E = (call (E)*?)");
    Query::expect_invalid("E = (call (E)+?)");
}

#[test]
fn invalid_mandatory_recursion_in_tree_child() {
    let input = "E = (call (E))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | E = (call (E))
      |            ^
      |            |
      |            E references itself
    ");
}

#[test]
fn invalid_mandatory_recursion_in_field() {
    let input = "E = (call body: (E))";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | E = (call body: (E))
      |                  ^
      |                  |
      |                  E references itself
    ");
}

#[test]
fn invalid_mandatory_recursion_in_capture() {
    let input = "E = (call (E) @inner)";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | E = (call (E) @inner)
      |            ^
      |            |
      |            E references itself
    ");
}

#[test]
fn invalid_mandatory_recursion_in_sequence() {
    let input = "E = (call {(a) (E)})";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | E = (call {(a) (E)})
      |                 ^
      |                 |
      |                 E references itself
    ");
}

#[test]
fn valid_recursion_with_base_case_and_descent() {
    let input = "E = [(x) (call (a) (E))]";
    Query::expect_valid(input);
}

#[test]
fn invalid_mutual_recursion_without_base_case() {
    let input = indoc! {r#"
        A = (foo (B))
        B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (B))
      |           - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn valid_mutual_recursion_with_base_case() {
    let input = indoc! {r#"
        A = [(x) (foo (B))]
        B = (bar (A))
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_three_way_mutual_recursion() {
    let input = indoc! {r#"
        A = (a (B))
        B = (b (C))
        C = (c (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (a (B))
      |         - references B
    2 | B = (b (C))
      |         - references C (completing cycle)
    3 | C = (c (A))
      | -       ^
      | |       |
      | |       references A
      | C is defined here
    ");
}

#[test]
fn valid_three_way_mutual_recursion_with_base_case() {
    let input = indoc! {r#"
        A = [(x) (a (B))]
        B = (b (C))
        C = (c (A))
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_diamond_dependency_recursion() {
    let input = indoc! {r#"
        A = (a [(B) (C)])
        B = (b (D))
        C = (c (D))
        D = (d (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (a [(B) (C)])
      |              - references C (completing cycle)
    2 | B = (b (D))
    3 | C = (c (D))
      | -       ^
      | |       |
      | |       references D
      | C is defined here
    4 | D = (d (A))
      |         - references A
    ");
}

#[test]
fn invalid_mutual_recursion_via_field() {
    let input = indoc! {r#"
        A = (foo body: (B))
        B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo body: (B))
      |                 - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn invalid_mutual_recursion_via_capture() {
    let input = indoc! {r#"
        A = (foo (B) @cap)
        B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (B) @cap)
      |           - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn invalid_mutual_recursion_via_sequence() {
    let input = indoc! {r#"
        A = (foo {(x) (B)})
        B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo {(x) (B)})
      |                - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn valid_mutual_recursion_with_optional_quantifier() {
    let input = indoc! {r#"
        A = (foo (B)?)
        B = (bar (A))
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_mutual_recursion_with_plus_quantifier() {
    let input = indoc! {r#"
        A = (foo (B)+)
        B = (bar (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (B)+)
      |           - references B (completing cycle)
    2 | B = (bar (A))
      | -         ^
      | |         |
      | |         references A
      | B is defined here
    ");
}

#[test]
fn valid_non_recursive_reference() {
    let input = indoc! {r#"
        Leaf = (identifier)
        Tree = (call (Leaf))
    "#};
    Query::expect_valid(input);
}

#[test]
fn valid_entry_point_using_recursive_def() {
    let input = indoc! {r#"
        E = [(x) (call (E))]
        Q = (program (E))
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_direct_left_recursion_in_alternation() {
    let input = "E = [(E) (x)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | E = [(E) (x)]
      |       ^
      |       |
      |       references itself
    ");
}

#[test]
fn invalid_direct_right_recursion_in_alternation() {
    let input = "E = [(x) (E)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | E = [(x) (E)]
      |           ^
      |           |
      |           references itself
    ");
}

#[test]
fn invalid_direct_left_recursion_in_tagged_alternation() {
    let input = "E = [Left: (E) Right: (x)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | E = [Left: (E) Right: (x)]
      |             ^
      |             |
      |             references itself
    ");
}

#[test]
fn invalid_unguarded_left_recursion_branch() {
    let input = indoc! {r#"
        A = [(A) 'escape']
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | A = [(A) 'escape']
      |       ^
      |       |
      |       references itself
    ");
}

#[test]
fn invalid_unguarded_left_recursion_with_wildcard_alt() {
    let input = indoc! {r#"
        A = [(A) _]
     "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | A = [(A) _]
      |       ^
      |       |
      |       references itself
    ");
}

#[test]
fn invalid_unguarded_left_recursion_with_tree_alt() {
    let input = indoc! {r#"
        A = [(A) (leaf)]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | A = [(A) (leaf)]
      |       ^
      |       |
      |       references itself
    ");
}

#[test]
fn valid_recursion_guarded_by_anchor() {
    let input = indoc! {r#"
        A = (foo . [(A) (x)])
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_mandatory_recursion_direct_child() {
    let input = indoc! {r#"
        A = (foo (A))
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (A))
      |           ^
      |           |
      |           A references itself
    ");
}

#[test]
fn valid_recursion_with_capture_base_case() {
    let input = indoc! {r#"
        A = [(x)@cap (foo (A))]
    "#};
    Query::expect_valid(input);
}

#[test]
fn invalid_mandatory_recursion_nested_plus() {
    let input = indoc! {r#"
        A = (foo (A)+)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle has no escape path
      |
    1 | A = (foo (A)+)
      |           ^
      |           |
      |           A references itself
    ");
}

#[test]
fn invalid_simple_unguarded_recursion() {
    let input = indoc! {r#"
        A = [
          (foo)
          (A)
        ]
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    3 |   (A)
      |    ^
      |    |
      |    references itself
    ");
}

#[test]
fn invalid_unguarded_mutual_recursion_chain() {
    let input = indoc! {r#"
        A = [(B) (x)]
        B = (A)
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: infinite recursion: cycle consumes no input
      |
    1 | A = [(B) (x)]
      |       - references B (completing cycle)
    2 | B = (A)
      | -    ^
      | |    |
      | |    references A
      | B is defined here
    ");
}
