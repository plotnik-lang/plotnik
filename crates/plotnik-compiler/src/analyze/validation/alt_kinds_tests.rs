use crate::Query;

#[test]
fn tagged_alternation_valid() {
    let input = "Q = [A: (a) B: (b)]";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
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
    let input = "Q = [(a) (b)]";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
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
    let input = "Q = [A: (a) (b)]";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = [A: (a) (b)]
      |      -      ^^^
      |      |
      |      tagged branch here
      |
    help: use all labels for a tagged union, or none for a merged struct
    ");
}

#[test]
fn mixed_alternation_untagged_first() {
    let input = r#"
    Q = [
      (a)
      B: (b)
    ]
    "#;

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: cannot mix labeled and unlabeled branches
      |
    3 |       (a)
      |       ^^^
    4 |       B: (b)
      |       - tagged branch here
      |
    help: use all labels for a tagged union, or none for a merged struct
    ");
}

#[test]
fn nested_mixed_alternation() {
    let input = "Q = (call [A: (a) (b)])";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (call [A: (a) (b)])
      |            -      ^^^
      |            |
      |            tagged branch here
      |
    help: use all labels for a tagged union, or none for a merged struct
    ");
}

#[test]
fn multiple_mixed_alternations() {
    let input = "Q = (foo [A: (a) (b)] [C: (c) (d)])";

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @r"
    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (foo [A: (a) (b)] [C: (c) (d)])
      |           -      ^^^
      |           |
      |           tagged branch here
      |
    help: use all labels for a tagged union, or none for a merged struct

    error: cannot mix labeled and unlabeled branches
      |
    1 | Q = (foo [A: (a) (b)] [C: (c) (d)])
      |                        -      ^^^
      |                        |
      |                        tagged branch here
      |
    help: use all labels for a tagged union, or none for a merged struct
    ");
}

#[test]
fn single_branch_no_error() {
    let input = "Q = [A: (a)]";

    let res = Query::expect_valid_ast(input);

    insta::assert_snapshot!(res, @r"
    Root
      Def Q
        Alt
          Branch A:
            NamedNode a
    ");
}
