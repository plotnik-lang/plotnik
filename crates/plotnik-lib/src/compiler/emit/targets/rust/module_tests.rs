use plotnik_rt::{Limit, Nav};

use crate::compiler::QueryBuilder;
use crate::compiler::test_utils::synthetic_grammar;
use crate::compiler::{CodegenProvenance, RustCodegenConfig};
use crate::core::grammar::GrammarIdentity;

use super::module::{depth_expr, limit_expr, nav_expr};

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
        "Some(rt::decode_depth_auto(512))"
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
    let grammar = synthetic_grammar().clone().with_identity(identity.clone());
    let compiled = QueryBuilder::from_inline("Q = (program)")
        .compile(&grammar)
        .expect("test query compiles");

    let generated = compiled
        .emit(RustCodegenConfig::new().provenance(CodegenProvenance::Full))
        .expect("emission succeeds")
        .into_artifact()
        .expect("valid query generates a matcher")
        .into_source();

    assert!(generated.contains("// Grammar name: \"plotnik_synthetic\""));
    assert!(generated.contains(identity.sha256()));
    assert!(
        generated.contains("const GRAMMAR_SOURCE: &str = \"grammar fixtures/synthetic.json\";")
    );
    assert!(generated.contains("regenerate against the"));
    assert!(generated.contains("grammar.json belonging to the parser"));
}

#[test]
fn inspection_does_not_change_generated_matcher() {
    let plain = QueryBuilder::from_inline("Q = (program)")
        .compile(synthetic_grammar())
        .expect("plain test query compiles");
    let inspected = QueryBuilder::from_inline("Q = (program)")
        .compile(synthetic_grammar())
        .expect("second test query compiles");

    let plain = plain
        .emit(RustCodegenConfig::new())
        .expect("emission succeeds")
        .into_artifact()
        .expect("plain query generates a matcher")
        .into_source();
    let inspected = inspected
        .emit(RustCodegenConfig::new())
        .expect("emission succeeds")
        .into_artifact()
        .expect("second query generates a matcher")
        .into_source();

    assert_eq!(inspected, plain);
}

#[test]
fn normal_generation_omits_debug_json_surface() {
    let generated = generated_module("Q = (program) @root", RustCodegenConfig::new());

    assert!(!generated.contains("parse_to_json"));
    assert!(!generated.contains("rt::debug"));
}

#[test]
fn debug_generation_wraps_result_and_match_only_callables() {
    let generated = generated_module(
        "Q = (program) @root\nPredicate = (program)",
        RustCodegenConfig::new().debug(true),
    );

    assert_eq!(generated.matches("pub fn parse_to_json(").count(), 2);
    assert!(generated.contains("let Some(value) = Self::parse(tree, source)? else"));
    assert!(generated.contains("rt::debug::to_json(&rt::WithSource::new(&value, source))"));
    assert!(generated.contains("if !Self::matches(tree, source)?"));
    assert!(generated.contains("::std::string::String::from(\"null\")"));
    assert!(generated.contains("SerializeWithSource for Q"));
}

#[test]
fn debug_implies_serde_regardless_of_builder_order() {
    let generated = generated_module(
        "Q = (program) @root",
        RustCodegenConfig::new().debug(true).serde(false),
    );

    assert!(generated.contains("SerializeWithSource for Q"));
    assert!(generated.contains("pub fn parse_to_json("));
}

#[test]
fn generated_calls_route_ports_through_immutable_call_sites() {
    let generated = generated_module(
        r#"
        Body = [
          Rec: {(comment) (B)}
          Base: (comment)
        ]
        B = {(Body)?? (Body)?}
        Q = (program {
          (B)
          .!
          (comment) @rest
        })
        "#,
        RustCodegenConfig::new(),
    );

    assert!(generated.contains("fn call_return(call_site: u16, port: rt::PortId) -> u16"));
    assert!(generated.contains(", 1) =>"));
    assert!(generated.contains("eng.enter_frame(resume.call_site);"));
    assert!(generated.contains("rt::PortId::from_raw("));
    assert!(!generated.contains("ReturnOutcome"));
    assert!(!generated.contains("enter_split_frame"));
}

fn generated_module(query: &str, config: RustCodegenConfig) -> String {
    QueryBuilder::from_inline(query)
        .compile(synthetic_grammar())
        .expect("test query compiles")
        .emit(config)
        .expect("emission succeeds")
        .into_artifact()
        .expect("valid query generates a matcher")
        .into_source()
}
