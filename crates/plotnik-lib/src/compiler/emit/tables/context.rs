//! Shared borrowed inputs for the emit phases.

use crate::core::Interner;

use crate::compiler::analyze::refs::DependencyAnalysis;
use crate::compiler::analyze::grammar::GrammarBinding;
use crate::compiler::core::TypeAnalysis;

/// The analysis artifacts every emit phase reads. The compiled IR
/// (`CompileResult`) and the in-flight string table are threaded separately —
/// the IR because it is produced by a different stage, the string table because
/// phases extend it.
#[derive(Clone, Copy)]
pub struct EmitInput<'a> {
    pub interner: &'a Interner,
    pub type_ctx: &'a TypeAnalysis,
    pub dependency_analysis: &'a DependencyAnalysis,
    pub grammar: &'a GrammarBinding,
}
