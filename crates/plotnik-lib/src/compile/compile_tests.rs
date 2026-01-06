//! Integration tests for the compilation pipeline.

use super::*;
use crate::emit::StringTableBuilder;
use crate::query::QueryBuilder;

#[test]
fn compile_simple_named_node() {
    let query = QueryBuilder::one_liner("Test = (identifier)")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    // Should have at least one instruction
    assert!(!result.instructions.is_empty());
    // Should have one entrypoint
    assert_eq!(result.def_entries.len(), 1);
}

#[test]
fn compile_alternation() {
    let query = QueryBuilder::one_liner("Test = [(identifier) (number)]")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}

#[test]
fn compile_sequence() {
    let query = QueryBuilder::one_liner("Test = {(comment) (function)}")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}

#[test]
fn compile_quantified() {
    let query = QueryBuilder::one_liner("Test = (identifier)*")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}

#[test]
fn compile_capture() {
    let query = QueryBuilder::one_liner("Test = (identifier) @id")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}

#[test]
fn compile_nested() {
    let query = QueryBuilder::one_liner("Test = (call_expression function: (identifier) @fn)")
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}

#[test]
fn compile_large_tagged_alternation() {
    // Regression test: alternations with 30+ branches should compile
    // by splitting epsilon transitions into a cascade.
    let branches: String = (0..30)
        .map(|i| format!("A{i}: (identifier) @x{i}"))
        .collect::<Vec<_>>()
        .join(" ");
    let query_str = format!("Q = [{branches}]");

    let query = QueryBuilder::one_liner(&query_str)
        .parse()
        .unwrap()
        .analyze();

    let mut strings = StringTableBuilder::new();
    let result = Compiler::compile(
        query.interner(),
        query.type_context(),
        &query.symbol_table,
        &mut strings,
        None,
        None,
    )
    .unwrap();

    assert!(!result.instructions.is_empty());
}
