//! Test utilities.

use std::sync::LazyLock;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::core::grammar::{Grammar, raw::RawGrammar};
use indexmap::IndexMap;

#[path = "../../test_support/grammar_loader.rs"]
mod grammar_loader;

pub fn empty_symbol_table() -> SymbolTable {
    SymbolTable::new(IndexMap::new(), IndexMap::new())
}

pub fn empty_dependency_analysis() -> DependencyAnalysis {
    DependencyAnalysis::empty()
}

pub fn javascript_grammar() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(&grammar_loader::load_arborium_grammar_json(
            "arborium-javascript",
        ))
        .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });

    &GRAMMAR
}

pub fn colliding_node_kind_grammar() -> Grammar {
    let raw = RawGrammar::from_json(
        r#"{
        "name": "collision",
        "rules": {
            "program": {
                "type": "CHOICE",
                "members": [
                    { "type": "SYMBOL", "name": "number" },
                    { "type": "STRING", "value": "number" }
                ]
            },
            "number": { "type": "STRING", "value": "literal" }
        }
    }"#,
    )
    .expect("collision grammar fixture");

    Grammar::from_raw(&raw).expect("collision grammar metadata")
}
