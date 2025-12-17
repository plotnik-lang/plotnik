use plotnik_core::{NodeFieldId, NodeTypeId};
use plotnik_langs::{Lang, javascript};

use crate::engine::interpreter::QueryInterpreter;
use crate::engine::value::Value;
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

fn run(query_src: &str, source: &str) -> String {
    let lang = javascript();

    // Parse, link, build graph
    let mut query = Query::new(query_src).exec().expect("query parse failed");

    if !query.is_valid() {
        return format!("QUERY ERROR:\n{}", query.diagnostics().render(query_src));
    }

    query.link(&lang);
    if !query.is_valid() {
        return format!("LINK ERROR:\n{}", query.diagnostics().render(query_src));
    }

    let query = query.build_graph();
    if query.has_type_errors() {
        return format!("TYPE ERROR:\n{}", query.diagnostics().render(query_src));
    }

    // Emit compiled query
    let resolver = LangResolver(lang.clone());
    let emitter = QueryEmitter::new(query.graph(), query.type_info(), resolver);
    let compiled = match emitter.emit() {
        Ok(c) => c,
        Err(e) => return format!("EMIT ERROR: {:?}", e),
    };

    // Parse source
    let tree = lang.parse(source);
    let cursor = tree.walk();

    // Run interpreter
    let interpreter = QueryInterpreter::new(&compiled, cursor, source);
    match interpreter.run() {
        Ok(value) => format_value(&value),
        Err(e) => format!("RUNTIME ERROR: {}", e),
    }
}

fn format_value(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|e| format!("JSON ERROR: {}", e))
}

#[test]
fn capture_identifier() {
    // AST: (program (expression_statement (identifier "x")))
    let query = "(program (expression_statement (identifier) @name))";
    let src = "x";

    let result = run(query, src);

    insta::assert_snapshot!(result, @r#"
    {
      "kind": "identifier",
      "text": "x",
      "range": [
        0,
        1
      ]
    }
    "#);
}

#[test]
fn capture_number() {
    // AST: (program (expression_statement (number "42")))
    let query = "(program (expression_statement (number) @num))";
    let src = "42";

    let result = run(query, src);

    insta::assert_snapshot!(result, @r#"
    {
      "kind": "number",
      "text": "42",
      "range": [
        0,
        2
      ]
    }
    "#);
}

#[test]
fn no_match_wrong_root() {
    // Query expects function_declaration at root, but AST root is program
    let query = "(function_declaration) @fn";
    let src = "function foo() {}";

    let result = run(query, src);

    insta::assert_snapshot!(result, @"null");
}
