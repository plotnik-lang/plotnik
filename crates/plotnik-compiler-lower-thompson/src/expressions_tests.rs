use std::num::NonZeroU16;

use indexmap::IndexMap;
use plotnik_core::{Interner, NodeKind};

use plotnik_compiler_core::ir::NodeKindConstraint;
use plotnik_compiler_core::{DependencyAnalysis, GrammarBinding, SymbolTable, TypeAnalysisBuilder};

use crate::{CompileCtx, Compiler};

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
    let symbol_table = SymbolTable::new(IndexMap::new(), IndexMap::new());
    let node_fields = IndexMap::new();
    let grammar = GrammarBinding::new(node_kinds, node_fields);
    let dependency_analysis = DependencyAnalysis::default();
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
