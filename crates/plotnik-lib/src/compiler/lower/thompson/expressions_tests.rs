use std::num::NonZeroU16;

use crate::core::{Interner, NodeKind};
use indexmap::IndexMap;

use crate::compiler::lower::ir::NodeKindConstraint;
use crate::compiler::core::{GrammarBinding, TypeAnalysisBuilder};
use crate::compiler::test_utils::{empty_dependency_analysis, empty_symbol_table};

use crate::compiler::lower::thompson::{CompileCtx, Compiler};

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
    let node_fields = IndexMap::new();
    let grammar = GrammarBinding::new(node_kinds, node_fields);
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
