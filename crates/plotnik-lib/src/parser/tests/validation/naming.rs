//! Naming validation tests: capture names, definition names, branch labels, field names, type names.

use crate::Query;
use indoc::indoc;

// =============================================================================
// Capture names
// =============================================================================

#[test]
fn capture_dotted_error() {
    let input = indoc! {r#"
    (identifier) @foo.bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_dotted_multiple_parts() {
    let input = indoc! {r#"
    (identifier) @a.b.c.d
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_dotted_followed_by_field() {
    let input = indoc! {r#"
    (call
        (identifier) @foo.bar
        name: (identifier))
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_hyphenated_error() {
    let input = indoc! {r#"
    (identifier) @foo-bar
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_hyphenated_multiple() {
    let input = indoc! {r#"
    (identifier) @a-b-c-d
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_mixed_dots_and_hyphens() {
    let input = indoc! {r#"
    (identifier) @foo.bar-baz
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");
}

#[test]
fn capture_name_pascal_case_error() {
    let input = indoc! {r#"
    (identifier) @MyCapture
    "#};

    let query = Query::new(input).unwrap();
    assert!(!query.is_valid());
    insta::assert_snapshot!(query.dump_errors(), @"");