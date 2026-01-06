use crate::utils::{to_pascal_case, to_snake_case};

#[test]
fn pascal_case_from_snake() {
    assert_eq!(to_pascal_case("foo_bar"), "FooBar");
    assert_eq!(to_pascal_case("foo"), "Foo");
    assert_eq!(to_pascal_case("_foo"), "Foo");
    assert_eq!(to_pascal_case("foo_"), "Foo");
}

#[test]
fn pascal_case_normalizes() {
    assert_eq!(to_pascal_case("FOO_BAR"), "FooBar");
    assert_eq!(to_pascal_case("FOO"), "Foo");
    assert_eq!(to_pascal_case("FOOBAR"), "Foobar");
}

#[test]
fn pascal_case_idempotent() {
    assert_eq!(to_pascal_case("FooBar"), "FooBar");
    assert_eq!(to_pascal_case("QRow"), "QRow");
    assert_eq!(to_pascal_case("Q"), "Q");
}

#[test]
fn pascal_case_from_kebab() {
    assert_eq!(to_pascal_case("foo-bar"), "FooBar");
    assert_eq!(to_pascal_case("foo-bar-baz"), "FooBarBaz");
}

#[test]
fn pascal_case_from_dotted() {
    assert_eq!(to_pascal_case("foo.bar"), "FooBar");
}

#[test]
fn snake_case_from_pascal() {
    assert_eq!(to_snake_case("FooBar"), "foo_bar");
    assert_eq!(to_snake_case("Foo"), "foo");
}

#[test]
fn snake_case_from_camel() {
    assert_eq!(to_snake_case("fooBar"), "foo_bar");
    assert_eq!(to_snake_case("fooBarBaz"), "foo_bar_baz");
}
