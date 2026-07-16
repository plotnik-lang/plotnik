//! Unified type checking pass.
//!
//! Computes static root extent and result flow in one traversal.

mod infer;
mod root_extent;
mod unify;

#[cfg(test)]
mod analysis_tests;
#[cfg(test)]
mod unify_tests;

pub use crate::compiler::analyze::types::RootExtent;
pub use crate::compiler::analyze::types::type_analysis::TypeAnalysis;
pub use crate::core::Interner;
pub(crate) use infer::definition_value_root;

use crate::compiler::analyze::names::SymbolTable;
use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::shape::anchor_context::AnchorContextAnalysis;
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
    let anchor_contexts = AnchorContextAnalysis::new(interner, symbol_table, dependency_analysis);
    let structural_facts = infer::StructuralFacts::analyze(
        interner,
        symbol_table,
        dependency_analysis,
        &anchor_contexts,
    );

    // One syntax-only O(AST) pre-scan buys the common path zero provenance
    // allocation. Folding this flag into inference would still make every
    // pattern result carry producer state even when absent; keep that cost confined to
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
        infer::freeze_field_completions(&mut types);
        return types.finish();
    }

    RawTypeNameValidator::new(&mut types, interner).validate(symbol_table, dependency_analysis);
    types.normalize_capture_types(interner, diag);
    TypeNamer::new(&mut types, interner, diag).assign(symbol_table, dependency_analysis);
    types.finish()
}

fn pattern_has_builtin_capture_type(pattern: &Pattern) -> bool {
    if let Pattern::CapturedPattern(captured_pattern) = pattern
        && let Some(name) = captured_pattern
            .capture()
            .capture_type()
            .and_then(|syntax| syntax.name())
        && BuiltInCaptureType::parse(name.text()).is_some()
    {
        return true;
    }
    pattern
        .children()
        .any(|child| pattern_has_builtin_capture_type(&child))
}
