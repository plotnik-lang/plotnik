#![allow(dead_code)]

pub fn dart_grammar_json() -> &'static str {
    include_str!(env!("PLOTNIK_TEST_GRAMMAR_DART"))
}

pub fn javascript_grammar_json() -> &'static str {
    include_str!(env!("PLOTNIK_TEST_GRAMMAR_JAVASCRIPT"))
}

pub fn typescript_grammar_json() -> &'static str {
    include_str!(env!("PLOTNIK_TEST_GRAMMAR_TYPESCRIPT"))
}
