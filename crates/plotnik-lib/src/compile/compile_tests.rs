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

#[test]
fn compile_unlabeled_alternation_5_branches_with_captures() {
    // Regression test: unlabeled alternation with 5+ branches where each has
    // a unique capture requires 8+ pre-effects (4 nulls + 4 sets per branch).
    // This exceeds the 3-bit limit (max 7) and must cascade via epsilon chain.
    let query = QueryBuilder::one_liner(
        "Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d (call_expression) @e]",
    )
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

    // Verify that effects cascade created extra epsilon instructions.
    // With 5 branches, each branch needs 8 pre-effects (4 missing captures × 2 effects).
    // This requires at least one cascade step per branch.
    let epsilon_count = result
        .instructions
        .iter()
        .filter(|i| matches!(i, crate::bytecode::InstructionIR::Match(m) if m.is_epsilon()))
        .count();

    // Should have more epsilon transitions than without cascade
    // (5 branches + cascade steps for overflow effects)
    assert!(epsilon_count >= 5, "expected cascade epsilon steps");
}

#[test]
fn compile_unlabeled_alternation_8_branches_with_captures() {
    // Even more extreme: 8 branches means 14 pre-effects per branch (7 nulls + 7 sets).
    // This requires 2 cascade steps per branch.
    let query = QueryBuilder::one_liner(
        "Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d \
              (call_expression) @e (member_expression) @f (array) @g (object) @h]",
    )
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
