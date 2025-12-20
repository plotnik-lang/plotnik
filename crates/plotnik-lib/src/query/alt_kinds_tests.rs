use crate::Query;

#[test]
fn tagged_alternation_valid() {
    let query = Query::try_from("Q = [A: (a) B: (b)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        Alt
          Branch A:
            NamedNode a
          Branch B:
            NamedNode b
    ");
}

#[test]
fn untagged_alternation_valid() {
    let query = Query::try_from("Q = [(a) (b)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        Alt
          Branch
            NamedNode a
          Branch
            NamedNode b
    ");
}

#[test]
fn mixed_alternation_tagged_first() {
    let query = Query::try_from("Q = [A: (a) (b)]").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = [A: (a) (b)]
      |      -      ^^^
      |      |
      |      tagged branch here
    ");
}

#[test]
fn mixed_alternation_untagged_first() {
    let query = Query::try_from(
        r#"
    Q = [
      (a)
      B: (b)
    ]
    "#,
    )
    .unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: cannot mix labeled and unlabeled branches
      |
    3 |       (a)
      |       ^^^
    4 |       B: (b)
      |       - tagged branch here
    ");
}

#[test]
fn nested_mixed_alternation() {
    let query = Query::try_from("Q = (call [A: (a) (b)])").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (call [A: (a) (b)])
      |            -      ^^^
      |            |
      |            tagged branch here
    ");
}

#[test]
fn multiple_mixed_alternations() {
    let query = Query::try_from("Q = (foo [A: (a) (b)] [C: (c) (d)])").unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_diagnostics(), @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (foo [A: (a) (b)] [C: (c) (d)])
      |           -      ^^^
      |           |
      |           tagged branch here

    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (foo [A: (a) (b)] [C: (c) (d)])
      |                        -      ^^^
      |                        |
      |                        tagged branch here
    ");
}

#[test]
fn single_branch_no_error() {
    let query = Query::try_from("Q = [A: (a)]").unwrap();
    assert!(query.is_valid());
    insta::assert_snapshot!(query.dump_ast(), @r"
    Root
      Def Q
        Alt
          Branch A:
            NamedNode a
    ");
}
