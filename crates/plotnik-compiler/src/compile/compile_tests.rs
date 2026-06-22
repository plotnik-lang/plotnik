use std::num::NonZeroU16;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeKind};

use crate::analyze::symbol_table::SymbolTableBuilder;
use crate::analyze::type_check::TypeContext;
use crate::bytecode::NodeKindConstraint;
use crate::compile::{CompileCtx, Compiler};
use crate::shot_bytecode;
use plotnik_compiler_core::GrammarBinding;

#[test]
fn compile_simple_named_node() {
    shot_bytecode!("Test = (identifier)");
}

#[test]
fn compile_alternation() {
    shot_bytecode!("Test = [(identifier) (number)]");
}

#[test]
fn resolve_anonymous_node_kind_uses_anonymous_namespace() {
    let mut interner = Interner::new();
    let number = interner.intern("number");
    let named_id = NonZeroU16::new(1).unwrap();
    let anonymous_id = NonZeroU16::new(2).unwrap();
    let node_kinds = IndexMap::from([
        (NodeKind::Named(number), named_id),
        (NodeKind::Anonymous(number), anonymous_id),
    ]);
    let type_ctx = TypeContext::new();
    let symbol_table = SymbolTableBuilder::new().finish();
    let node_fields = IndexMap::new();
    let grammar = GrammarBinding::new(node_kinds, node_fields);
    let ctx = CompileCtx {
        interner: &interner,
        type_ctx: &type_ctx,
        symbol_table: &symbol_table,
        grammar: &grammar,
    };
    let mut compiler = Compiler::new(&ctx);

    assert_eq!(
        compiler.resolve_anonymous_node_kind("number"),
        NodeKindConstraint::Anonymous(Some(anonymous_id))
    );
}

#[test]
fn compile_sequence() {
    shot_bytecode!("Test = {(comment) (identifier)}");
}

#[test]
fn compile_quantified() {
    shot_bytecode!("Test = (identifier)*");
}

#[test]
fn compile_capture() {
    shot_bytecode!("Test = (identifier) @id");
}

#[test]
fn compile_nested() {
    shot_bytecode!("Test = (call_expression function: (identifier) @fn)");
}

// Split-exit captures at a navigating first-child position (`?`/`*` whose skip
// path must restore the cursor). These pin the bytecode of the unified
// exit-aware emitters — `Struct`/`Arr`/`Suppress` opening once and closing on both
// the match and skip exits — so an effect-ordering regression is caught here even
// when it still produces the right conformance JSON on a narrow input (#470).

/// Struct capture: `Struct → inner → EndStruct+Set`, one EndStruct per exit.
#[test]
fn compile_optional_struct_capture_split_exits() {
    shot_bytecode!("Test = (program {(identifier) @id}? @outer)");
}

/// Array capture: `Arr → loop(Push) → EndArr+Set`, one EndArr per exit.
#[test]
fn compile_optional_array_capture_split_exits() {
    shot_bytecode!("Test = (program (expression_statement)* @rows)");
}

/// Suppressive capture: `SuppressBegin → inner → SuppressEnd`, one SuppressEnd per
/// exit; emits no value despite the inner's struct mechanism.
#[test]
fn compile_optional_suppressed_capture_split_exits() {
    shot_bytecode!("Test = (program {(identifier) @id}? @_)");
}

#[test]
fn compile_large_enum_alternation() {
    // Regression test: alternations with 30+ branches should compile
    // by splitting epsilon transitions into a cascade.
    shot_bytecode!(
        r#"
        Q = [
            A0: (identifier) @x0   A1: (identifier) @x1   A2: (identifier) @x2
            A3: (identifier) @x3   A4: (identifier) @x4   A5: (identifier) @x5
            A6: (identifier) @x6   A7: (identifier) @x7   A8: (identifier) @x8
            A9: (identifier) @x9   A10: (identifier) @x10 A11: (identifier) @x11
            A12: (identifier) @x12 A13: (identifier) @x13 A14: (identifier) @x14
            A15: (identifier) @x15 A16: (identifier) @x16 A17: (identifier) @x17
            A18: (identifier) @x18 A19: (identifier) @x19 A20: (identifier) @x20
            A21: (identifier) @x21 A22: (identifier) @x22 A23: (identifier) @x23
            A24: (identifier) @x24 A25: (identifier) @x25 A26: (identifier) @x26
            A27: (identifier) @x27 A28: (identifier) @x28 A29: (identifier) @x29
        ]
    "#
    );
}

#[test]
fn compile_union_alternation_5_branches_with_captures() {
    // Regression test: union alternation with 5+ branches where each has
    // a unique capture requires 8+ pre-effects (4 nulls + 4 sets per branch).
    // This exceeds the 3-bit limit (max 7) and must cascade via epsilon chain.
    shot_bytecode!(
        "Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d (call_expression) @e]"
    );
}

#[test]
fn compile_union_alternation_8_branches_with_captures() {
    // Even more extreme: 8 branches means 14 pre-effects per branch (7 nulls + 7 sets).
    // This requires 2 cascade steps per branch.
    shot_bytecode!(
        r#"
        Q = [(identifier) @a (number) @b (string) @c (binary_expression) @d
             (call_expression) @e (member_expression) @f (array) @g (object) @h]
    "#
    );
}
