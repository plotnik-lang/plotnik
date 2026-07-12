#![allow(dead_code)]

pub fn load_arborium_grammar_json(package: &str) -> &'static str {
    match package {
        "arborium-javascript" => include_str!(env!("PLOTNIK_TEST_GRAMMAR_JAVASCRIPT")),
        "arborium-typescript" => include_str!(env!("PLOTNIK_TEST_GRAMMAR_TYPESCRIPT")),
        "arborium-dart" => include_str!(env!("PLOTNIK_TEST_GRAMMAR_DART")),
        _ => panic!("{package} grammar is not embedded in plotnik-tests"),
    }
}
