use crate::Query;
use indoc::indoc;

#[test]
fn valid_type_inference() {
    let input = indoc! {r#"
        ... defs ...

        "Q = ...
    "#};

    let res = Query::expect_valid_types(input);

    insta::assert_snapshot!(res, @"");
}

#[test]
fn invalid_type_inference() {
    let input = indoc! {r#"
        ... defs ...

        "Q = ...
    "#};

    let res = Query::expect_invalid(input);

    insta::assert_snapshot!(res, @"");
}
