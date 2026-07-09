use plotnik_rt::{Limit, Nav};

use super::emit::names::{shouty_ident, snake_ident};
use super::emitter::{depth_expr, limit_expr, nav_expr};

#[test]
fn shouty_splits_pascal_humps() {
    assert_eq!(shouty_ident("FooBar"), "FOO_BAR");
    assert_eq!(shouty_ident("Q"), "Q");
    assert_eq!(shouty_ident("HTTPServer"), "HTTP_SERVER");
    assert_eq!(shouty_ident("Foo2Bar"), "FOO2_BAR");
}

#[test]
fn snake_splits_pascal_humps() {
    assert_eq!(snake_ident("FooBar"), "foo_bar");
    assert_eq!(snake_ident("Q"), "q");
    assert_eq!(snake_ident("HTTPServer"), "http_server");
}

#[test]
fn nav_expr_matches_debug() {
    // Generated code spells navs via their Debug form; pin that Debug output
    // stays valid variant syntax, including the tuple variants.
    assert_eq!(nav_expr(Nav::DownSkip), "rt::Nav::DownSkip");
    assert_eq!(nav_expr(Nav::Up(2)), "rt::Nav::Up(2)");
    assert_eq!(nav_expr(Nav::UpSkipTrivia(31)), "rt::Nav::UpSkipTrivia(31)");
}

#[test]
fn limit_expr_matches_debug() {
    // The compiled-in `LIMITS` const spells limits via their Debug form; pin
    // that it stays valid variant syntax, including the tuple variant.
    assert_eq!(limit_expr(Limit::Auto), "rt::Limit::Auto");
    assert_eq!(limit_expr(Limit::Of(3)), "rt::Limit::Of(3)");
    assert_eq!(limit_expr(Limit::Unbounded), "rt::Limit::Unbounded");
}

#[test]
fn depth_expr_resolves_at_generation_time() {
    assert_eq!(
        depth_expr(Limit::Auto, 512),
        "Some(rt::replay_depth_auto(512))"
    );
    assert_eq!(depth_expr(Limit::Of(8), 512), "Some(8)");
    assert_eq!(depth_expr(Limit::Unbounded, 512), "None");
}
