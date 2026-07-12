#![allow(dead_code)]

pub mod atomic_file;
pub mod formatter;
pub mod snapshots;

#[path = "../test_support/grammar_loader.rs"]
mod grammar_loader;

use std::sync::LazyLock;

use plotnik_lib::grammar::{Grammar, raw::RawGrammar};
use tree_sitter::{Language as TsLanguage, Parser as TsParser, Tree};

pub use grammar_loader::load_arborium_grammar_json;

pub fn javascript_grammar() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(load_arborium_grammar_json("arborium-javascript"))
            .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });

    &GRAMMAR
}

pub fn parse_javascript(source: &str) -> Tree {
    let mut parser = TsParser::new();
    let lang: TsLanguage = arborium_javascript::language().into();
    parser.set_language(&lang).expect("set javascript language");
    parser.parse(source, None).expect("parse javascript source")
}
