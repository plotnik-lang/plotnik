#[cfg(any(feature = "lang-javascript", feature = "lang-typescript"))]
fn capture_name(language: &str, source: &str) -> serde_json::Value {
    use plotnik_compiler::{BytecodeConfig, Colors, QueryBuilder, VM, materialize_verified};

    let lang = super::langs::resolve(language).unwrap();
    let query = QueryBuilder::from_inline(
        "Q = (program (lexical_declaration (variable_declarator name: (identifier) @name :: text)))",
    )
    .compile(lang.grammar())
    .unwrap();
    let module = query
        .emit(BytecodeConfig::new())
        .unwrap()
        .into_artifact()
        .unwrap();
    let entry = module.entrypoint("Q").unwrap();
    let tree = lang.parse_source(source);
    assert!(!tree.root_node().has_error());

    let journal = VM::new(source, tree.root_node())
        .execute(&module, &entry)
        .unwrap();
    let value = materialize_verified(
        source,
        &module,
        &entry,
        journal.output_events(),
        Colors::new(false),
    );
    serde_json::to_value(value).unwrap()
}

#[cfg(feature = "lang-javascript")]
#[test]
fn javascript_parser_matches_embedded_grammar() {
    let result = capture_name("javascript", "const value = 1;");

    assert_eq!(result, serde_json::json!({"name": "value"}));
    assert_eq!(
        super::langs::resolve("javascript")
            .unwrap()
            .identity()
            .name(),
        "javascript"
    );
}

#[cfg(feature = "lang-typescript")]
#[test]
fn typescript_parser_matches_embedded_grammar() {
    let result = capture_name("typescript", "const value: number = 1;");

    assert_eq!(result, serde_json::json!({"name": "value"}));
    assert_eq!(
        super::langs::resolve("typescript")
            .unwrap()
            .identity()
            .name(),
        "typescript"
    );
}
