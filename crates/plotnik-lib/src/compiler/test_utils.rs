//! Test utilities.

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::core::grammar::{Grammar, raw::RawGrammar};
use indexmap::IndexMap;

pub fn empty_symbol_table() -> SymbolTable {
    SymbolTable::new(IndexMap::new(), IndexMap::new())
}

pub fn empty_dependency_analysis() -> DependencyAnalysis {
    DependencyAnalysis::empty()
}

pub fn javascript_grammar() -> &'static Grammar {
    static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
        let raw = RawGrammar::from_json(&arborium_grammar_json("arborium-javascript"))
            .expect("javascript grammar fixture");
        Grammar::from_raw(&raw).expect("javascript grammar metadata")
    });

    &GRAMMAR
}

fn arborium_grammar_json(package: &str) -> String {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(manifest_path)
        .exec()
        .expect("cargo metadata should resolve dev-dependencies");
    let found = metadata
        .packages
        .iter()
        .find(|p| p.name == package)
        .unwrap_or_else(|| panic!("{package} package not found"));
    let root = found
        .manifest_path
        .parent()
        .unwrap_or_else(|| panic!("{package} package has no parent dir"));
    let grammar_path = root.join("grammar/src/grammar.json");
    fs::read_to_string(&grammar_path)
        .unwrap_or_else(|e| panic!("{package} grammar.json not found at {grammar_path}: {e}"))
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
