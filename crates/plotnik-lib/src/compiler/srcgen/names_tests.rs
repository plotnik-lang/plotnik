use crate::compiler::srcgen::names::{rust_scope_idents, shouty_ident, snake_ident};

#[test]
fn case_conversion_preserves_acronym_boundaries() {
    assert_eq!(snake_ident("FooBar"), "foo_bar");
    assert_eq!(shouty_ident("FooBar"), "FOO_BAR");
    assert_eq!(snake_ident("Q"), "q");
    assert_eq!(shouty_ident("Q"), "Q");
    assert_eq!(snake_ident("HTTPServer"), "http_server");
    assert_eq!(shouty_ident("HTTPServer"), "HTTP_SERVER");
    assert_eq!(shouty_ident("Foo2Bar"), "FOO2_BAR");
    assert_eq!(snake_ident("Foo_Bar"), "foo_bar");
}

#[test]
fn rust_keywords_are_hygienic_within_one_scope() {
    let names = ["type", "self", "self_", "ordinary"];

    assert_eq!(
        rust_scope_idents(names.into_iter()),
        ["r#type", "self_", "self__", "ordinary"]
    );
}
