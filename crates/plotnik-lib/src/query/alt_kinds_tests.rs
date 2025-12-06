use crate::Query;

#[test]
fn tagged_alternation_valid() {
    let query = Query::try_from("[A: (a) B: (b)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        Alt
          Branch A:
            NamedNode a
          Branch B:
            NamedNode b
    ");
}

#[test]
fn untagged_alternation_valid() {
    let query = Query::try_from("[(a) (b)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        Alt
          Branch
            NamedNode a
          Branch
            NamedNode b
    ");
}

#[test]
fn mixed_alternation_tagged_first() {
    let query = Query::try_from("[A: (a) (b)]").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: mixed tagged and untagged branches in alternation
      |
    1 | [A: (a) (b)]
      |  -      ^^^
      |  |
      |  tagged branch here
    ");
}

#[test]
fn mixed_alternation_untagged_first() {
    let query = Query::try_from(
        r#"
    [
      (a)
      B: (b)
    ]
    "#,
    )
    .unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: mixed tagged and untagged branches in alternation
      |
    3 |       (a)
      |       ^^^
    4 |       B: (b)
      |       - tagged branch here
    ");
}

#[test]
fn nested_mixed_alternation() {
    let query = Query::try_from("(call [A: (a) (b)])").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: mixed tagged and untagged branches in alternation
      |
    1 | (call [A: (a) (b)])
      |        -      ^^^
      |        |
      |        tagged branch here
    ");
}

#[test]
fn multiple_mixed_alternations() {
    let query = Query::try_from("(foo [A: (a) (b)] [C: (c) (d)])").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: mixed tagged and untagged branches in alternation
      |
    1 | (foo [A: (a) (b)] [C: (c) (d)])
      |       -      ^^^
      |       |
      |       tagged branch here

    error: mixed tagged and untagged branches in alternation
      |
    1 | (foo [A: (a) (b)] [C: (c) (d)])
      |                    -      ^^^
      |                    |
      |                    tagged branch here
    ");
}

#[test]
fn single_branch_no_error() {
    let query = Query::try_from("[A: (a)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def
        Alt
          Branch A:
            NamedNode a
    ");
}
