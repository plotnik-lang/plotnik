use std::num::NonZeroU16;

use crate::core::{Interner, NodeKind};
use indexmap::IndexMap;

use crate::compiler::analyze::grammar::GrammarBindingBuilder;
use crate::compiler::analyze::types::type_analysis::TypeAnalysisBuilder;
use crate::compiler::lower::context::CompileCtx;
use crate::compiler::lower::ir::NodeKindConstraint;
use crate::compiler::test_utils::{empty_dependency_analysis, empty_symbol_table};

use super::Compiler;

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
    let type_ctx = TypeAnalysisBuilder::new().finish();
    let symbol_table = empty_symbol_table();
    let mut grammar_builder = GrammarBindingBuilder::new();
    for (kind, id) in node_kinds {
        grammar_builder.insert_node_kind_id(kind, id);
    }
    let grammar = grammar_builder.finish();
    let dependency_analysis = empty_dependency_analysis();
    let ctx = CompileCtx {
        interner: &interner,
        type_ctx: &type_ctx,
        symbol_table: &symbol_table,
        grammar: &grammar,
        dependency_analysis: &dependency_analysis,
    };
    let mut compiler = Compiler::new(&ctx);

    assert_eq!(
        compiler.resolve_anonymous_node_kind("number"),
        NodeKindConstraint::Anonymous(Some(anonymous_id))
    );
}

#[test]
#[should_panic(expected = "linked anonymous token kind must be bound")]
fn resolve_anonymous_node_kind_requires_linked_binding() {
    let mut interner = Interner::new();
    interner.intern("number");
    let type_ctx = TypeAnalysisBuilder::new().finish();
    let symbol_table = empty_symbol_table();
    let grammar = GrammarBindingBuilder::new().finish();
    let dependency_analysis = empty_dependency_analysis();
    let ctx = CompileCtx {
        interner: &interner,
        type_ctx: &type_ctx,
        symbol_table: &symbol_table,
        grammar: &grammar,
        dependency_analysis: &dependency_analysis,
    };
    let mut compiler = Compiler::new(&ctx);

    compiler.resolve_anonymous_node_kind("number");
}

#[test]
#[should_panic(expected = "linked field name must be bound")]
fn resolve_field_by_name_requires_linked_binding() {
    let mut interner = Interner::new();
    interner.intern("name");
    let type_ctx = TypeAnalysisBuilder::new().finish();
    let symbol_table = empty_symbol_table();
    let grammar = GrammarBindingBuilder::new().finish();
    let dependency_analysis = empty_dependency_analysis();
    let ctx = CompileCtx {
        interner: &interner,
        type_ctx: &type_ctx,
        symbol_table: &symbol_table,
        grammar: &grammar,
        dependency_analysis: &dependency_analysis,
    };
    let mut compiler = Compiler::new(&ctx);

    compiler.resolve_field_by_name("name");
}
