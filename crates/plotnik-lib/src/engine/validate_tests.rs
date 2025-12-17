//! End-to-end tests for runtime type validation.

use plotnik_core::{NodeFieldId, NodeTypeId};
use plotnik_langs::{Lang, javascript};

use crate::engine::interpreter::QueryInterpreter;
use crate::engine::validate::validate;
use crate::ir::{NodeKindResolver, QueryEmitter};
use crate::query::Query;

struct LangResolver(Lang);

impl NodeKindResolver for LangResolver {
    fn resolve_kind(&self, name: &str) -> Option<NodeTypeId> {
        self.0.resolve_named_node(name)
    }

    fn resolve_field(&self, name: &str) -> Option<NodeFieldId> {
        self.0.resolve_field(name)
    }
}

fn run_and_validate(query_src: &str, source: &str) -> String {
    let lang = javascript();

    let mut query = Query::new(query_src).exec().expect("query parse failed");
    assert!(
        query.is_valid(),
        "query invalid: {}",
        query.diagnostics().render(query_src)
    );

    query.link(&lang);
    assert!(
        query.is_valid(),
        "link failed: {}",
        query.diagnostics().render(query_src)
    );

    let query = query.build_graph();
    assert!(
        !query.has_type_errors(),
        "type error: {}",
        query.diagnostics().render(query_src)
    );

    let resolver = LangResolver(lang.clone());
    let emitter = QueryEmitter::new(query.graph(), query.type_info(), resolver);
    let compiled = emitter.emit().expect("emit failed");

    let tree = lang.parse(source);
    let cursor = tree.walk();

    let interpreter = QueryInterpreter::new(&compiled, cursor, source);
    let result = interpreter.run().expect("runtime error");

    let expected_type = compiled.entrypoints().first().unwrap().result_type();

    match validate(&result, expected_type, &compiled) {
        Ok(()) => "OK".to_string(),
        Err(e) => format!("VALIDATION ERROR: {}", e),
    }
}

#[test]
fn validate_simple_capture() {
    let result = run_and_validate("(program (expression_statement (identifier) @name))", "x");
    insta::assert_snapshot!(result, @"OK");
}

#[test]
fn validate_string_annotation() {
    let result = run_and_validate(
        "(program (expression_statement (identifier) @name :: string))",
        "x",
    );
    insta::assert_snapshot!(result, @"OK");
}

#[test]
fn validate_sequence_star() {
    let result = run_and_validate(
        "(program { (expression_statement (identifier) @id)* })",
        "x; y; z",
    );
    insta::assert_snapshot!(result, @"OK");
}

#[test]
fn validate_sequence_plus() {
    let result = run_and_validate(
        "(program { (expression_statement (identifier) @id)+ })",
        "x; y",
    );
    insta::assert_snapshot!(result, @"OK");
}

#[test]
fn validate_optional_present() {
    let result = run_and_validate("(program (expression_statement (identifier)? @maybe))", "x");
    insta::assert_snapshot!(result, @"OK");
}

#[test]
fn validate_optional_absent() {
    let result = run_and_validate(
        "(program (expression_statement (number)? @maybe (identifier)))",
        "x",
    );
    insta::assert_snapshot!(result, @"OK");
}
