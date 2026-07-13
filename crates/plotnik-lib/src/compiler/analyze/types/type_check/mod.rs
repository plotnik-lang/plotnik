//! Unified type checking pass.
//!
//! Computes both structural arity (for field validation) and data flow types
//! (for TypeScript emission) in a single traversal.

mod def_arity;
mod infer;
mod unify;

#[cfg(test)]
mod analysis_tests;
#[cfg(test)]
mod unify_tests;

pub use crate::compiler::analyze::types::type_analysis::TypeAnalysis;
pub use crate::compiler::analyze::types::type_shape::Arity;
pub use crate::core::Interner;
pub(crate) use infer::consumable_value_root;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::types::BuiltInCaptureType;
use crate::compiler::analyze::types::naming::{RawTypeNameValidator, TypeNamer};
use crate::compiler::diagnostics::report::Diagnostics;
use crate::compiler::parse::ast::Pattern;

/// Run type inference on all definitions.
///
/// Processes definitions in dependency order (leaves first) to handle
/// recursive definitions correctly.
pub fn infer_types(
    interner: &mut Interner,
    symbol_table: &SymbolTable,
    dependency_analysis: &DependencyAnalysis,
    diag: &mut Diagnostics,
) -> TypeAnalysis {
    let structural_facts =
        infer::StructuralFacts::analyze(interner, symbol_table, dependency_analysis);

    // One syntax-only O(AST) pre-scan buys the common path zero provenance
    // allocation. Folding this flag into inference would still make every
    // pattern result carry optional producer state; keep that cost confined to
    // queries that actually write a builtin capture type.
    let has_capture_types = symbol_table
        .names()
        .filter_map(|name| symbol_table.body(name))
        .any(pattern_has_builtin_capture_type);
    let pass = infer::InferPassEnv {
        interner,
        symbol_table,
        dependency_analysis,
        structural_facts: &structural_facts,
        diag,
    };
    let mut types = if has_capture_types {
        infer::InferPass::normalizing_capture_types(pass).run()
    } else {
        infer::InferPass::new(pass).run()
    };

    if !has_capture_types {
        TypeNamer::new(&mut types, interner, diag).assign(symbol_table, dependency_analysis);
        infer::freeze_union_flow_plans(&mut types);
        return types.finish();
    }

    RawTypeNameValidator::new(&mut types, interner).validate(symbol_table, dependency_analysis);
    types.normalize_capture_types(interner, diag);
    TypeNamer::new(&mut types, interner, diag).assign(symbol_table, dependency_analysis);
    types.finish()
}

fn pattern_has_builtin_capture_type(pattern: &Pattern) -> bool {
    if let Pattern::CapturedPattern(capture) = pattern
        && let Some(name) = capture.capture_type().and_then(|syntax| syntax.name())
        && BuiltInCaptureType::parse(name.text()).is_some()
    {
        return true;
    }
    pattern
        .children()
        .any(|child| pattern_has_builtin_capture_type(&child))
}
