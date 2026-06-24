use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::TypeAnalysis;
use crate::compiler::lower::context::CompileCtx;
use crate::compiler::lower::dead::remove_unreachable;
use crate::compiler::lower::epsilon::eliminate_epsilons;
use crate::compiler::lower::ir::LoweredIr;
use crate::compiler::lower::nav::collapse_up;
use crate::compiler::lower::pack::lower;
use crate::compiler::lower::thompson::Compiler;
use crate::compiler::lower::verify::{run_verified, verify_constructed};
use crate::core::Interner;

mod context;
pub mod dead;
pub mod epsilon;
pub mod ir;
pub mod nav;
pub mod pack;
pub mod thompson;
mod verify;

#[cfg(test)]
mod ir_tests;

/// Inputs required by the lowering pipeline.
pub(crate) struct LowerInput<'a> {
    pub(crate) interner: &'a Interner,
    pub(crate) type_ctx: &'a TypeAnalysis,
    pub(crate) symbol_table: &'a SymbolTable,
    pub(crate) grammar: &'a GrammarBinding,
    pub(crate) dependency_analysis: &'a DependencyAnalysis,
}

pub(crate) fn lower_to_ir(input: LowerInput<'_>) -> LoweredIr {
    let ctx = CompileCtx {
        interner: input.interner,
        type_ctx: input.type_ctx,
        symbol_table: input.symbol_table,
        grammar: input.grammar,
        dependency_analysis: input.dependency_analysis,
    };

    let mut ir = Compiler::build_ir(&ctx);
    verify_constructed(&ir, &ctx);
    run_verified("eliminate_epsilons", &mut ir, &ctx, eliminate_epsilons);
    run_verified("remove_unreachable", &mut ir, &ctx, remove_unreachable);
    run_verified("collapse_up", &mut ir, &ctx, collapse_up);
    run_verified("lower", &mut ir, &ctx, lower);
    verify_constructed(&ir, &ctx);

    LoweredIr::new(ir)
}
