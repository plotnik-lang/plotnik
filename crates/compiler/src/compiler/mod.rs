//! Plotnik compiler pipeline.
//!
//! Orchestrates parsing, analysis, lowering, and target emission
//! into the `Query`/`QueryBuilder` pipeline. Pass internals stay under this
//! module; the crate root re-exports only the facade-level API.

mod analyze;
pub(crate) mod diagnostics;
mod fmt;
mod ids;
pub(crate) mod limits;
mod lower;
pub(crate) mod parse;
pub(crate) mod regex;

pub(crate) mod emit;
pub(crate) mod query;
#[cfg(test)]
pub mod test_utils;

pub use crate::compiler::diagnostics::source;

pub use crate::compiler::diagnostics::{
    DiagnosticBuilder, DiagnosticKind, Diagnostics, Error, QueryResult, Severity, Source, SourceId,
    SourceKind, SourceMap, SourcePath, Span,
};
pub use emit::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CodegenTarget, Emission,
    EmitConfigError, EmitTarget, RustCodegenConfig, RustModuleOutput, RustTypesOutput,
    TypeScriptBinding, TypeScriptCodegenConfig, TypeScriptMatchOnlyType,
    TypeScriptNodeRepresentation, TypeScriptTypesOutput, journal_fn_name,
};
pub use fmt::{FormatError, FormatResult, format_query};
pub use parse::{InputKind, QueryToken, parse_input_kind, tokenize};
pub use query::{
    CompiledQuery, GrammarDeclaration, GrammarDeclarationError, Query, QueryBuilder,
    SourceCollection, parse_grammar_declaration,
};
