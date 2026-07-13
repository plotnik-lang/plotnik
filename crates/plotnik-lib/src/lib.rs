//! Plotnik: a typed query language for Tree-sitter syntax trees.
//!
//! # Example
//!
//! ```
//! use plotnik_lib::Query;
//!
//! let source = r#"
//!     Expr = [(identifier) (number)]
//!     (assignment left: (Expr) @lhs right: (Expr) @rhs)
//! "#;
//!
//! let query = Query::try_from(source).expect("query compiles");
//! eprintln!("{}", query.diagnostics().render(query.source_map()));
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[doc(hidden)]
pub mod bytecode;
mod compiler;
mod core;
#[cfg(feature = "vm")]
mod vm;

pub use crate::bytecode::type_system;
pub use crate::core::colors;
pub use crate::core::grammar;
pub use crate::core::utils as text_utils;
pub use crate::core::{Cardinality, NodeFieldId, NodeKind, NodeKindId};
#[cfg(feature = "vm")]
pub use crate::core::{
    DumpChunk, DumpChunkKind, DumpNode, TreeDump, dump_tree, dump_tree_text, tree_to_json,
};

pub mod diagnostics {
    pub use crate::compiler::diagnostics::report::{
        DiagnosticBuilder, DiagnosticKind, Diagnostics, JsonDiagnostic, JsonFix, JsonPosition,
        JsonRelated, JsonSpan, Severity,
    };
    pub use crate::compiler::diagnostics::{
        Error, QueryResult, Source, SourceId, SourceKind, SourceMap, SourcePath, Span,
    };
}

pub use crate::compiler::entry_fn_name as matcher_entry_fn_name;
pub use crate::compiler::{TypeScriptBinding, TypeScriptMatchOnlyType};

pub use crate::core::Colors;
pub use crate::core::grammar::GrammarIdentity;

pub use crate::compiler::{
    BytecodeConfig, BytecodeInspection, CodegenProvenance, CodegenTarget, DiagnosticBuilder,
    DiagnosticKind, Diagnostics, Emission, EmitConfigError, EmitTarget, Error, FormatError,
    FormatResult, QueryResult, RustCodegenConfig, RustModuleOutput, RustTypesOutput, Severity,
    Span, TypeScriptCodegenConfig, TypeScriptNodeRepresentation, TypeScriptTypesOutput,
};
pub use crate::compiler::{
    CompiledQuery, Query, QueryBuilder, QueryToken, Source, SourceId, SourceKind, SourceMap,
    SourcePath, format_query, tokenize,
};

#[cfg(feature = "vm")]
pub use crate::vm::{
    ExecutionTrace, JournalEvent, Limit, MatchJournal, NodeValue, NoopTracer, PrintTracer,
    PrintTracerBuilder, ProvenanceBinding, ResolvedRuntimeLimits, ResultProvenance,
    ResultProvenanceEntry, RunStats, RuntimeError, RuntimeLimitSpec, TraceEvent, TraceNode,
    TraceRecord, TraceRecorder, Tracer, VM, VMBuilder, Value, ValueMaterializer, Verbosity,
    debug_verify_type, extract_result_provenance, materialize_verified,
};
