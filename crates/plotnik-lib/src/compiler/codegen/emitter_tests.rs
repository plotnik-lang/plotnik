use plotnik_rt::{Limit, Nav};

use crate::compiler::QueryBuilder;
use crate::compiler::codegen::{Config, GrammarIdentity};
use crate::compiler::test_utils::synthetic_grammar;

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

#[test]
fn generated_product_module_records_exact_grammar_identity() {
    let identity = GrammarIdentity::from_json_bytes(
        "plotnik_synthetic",
        b"{}",
        "grammar fixtures/synthetic.json",
    );
    assert_eq!(
        identity.sha256(),
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
    let compiled = QueryBuilder::from_inline("Q = (program)")
        .compile(synthetic_grammar())
        .expect("test query compiles");

    let generated = compiled
        .to_rust_matcher(Config::new().grammar_identity(identity.clone()))
        .expect("valid query generates a matcher");

    assert!(generated.contains("// Grammar name: \"plotnik_synthetic\""));
    assert!(generated.contains(identity.sha256()));
    assert!(
        generated.contains("const GRAMMAR_SOURCE: &str = \"grammar fixtures/synthetic.json\";")
    );
    assert!(generated.contains("regenerate against the"));
    assert!(generated.contains("grammar.json belonging to the parser"));
}
