//! Plotnik: Query language for tree-sitter AST with type inference.
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

pub mod bytecode;
mod compiler;
mod core;
mod vm;

pub use crate::bytecode::type_system;
pub use crate::core::colors;
pub use crate::core::grammar;
pub use crate::core::utils as text_utils;
pub use crate::core::{
    Cardinality, DumpChunk, DumpChunkKind, DumpNode, NodeFieldId, NodeKind, NodeKindId, TreeDump,
    dump_tree, dump_tree_text, tree_to_json,
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

pub use crate::compiler::typegen::typescript::{
    Config as TypeScriptConfig, DtsRange, VoidType as TypeScriptVoidType,
};

pub use crate::core::Colors;

pub use crate::compiler::{
    CheckedQuery, CompiledQuery, Query, QueryBuilder, Source, SourceId, SourceKind, SourceMap,
    SourcePath, TokenSpan, tokenize,
};
pub use crate::compiler::{
    DiagnosticBuilder, DiagnosticKind, Diagnostics, Error, QueryResult, Severity, Span,
};

pub use crate::vm::{
    Binding, EffectLog, Inspection, InspectionEntry, Limit, NodeHandle, NodeRef, NoopTracer,
    PrintTracer, PrintTracerBuilder, Recording, RecordingTracer, ResolvedRuntimeLimits, RunStats,
    RuntimeEffect, RuntimeError, RuntimeLimitSpec, StepEvent, StepRecord, Tracer, VM, VMBuilder,
    Value, ValueMaterializer, Verbosity, debug_verify_type, extract_inspection,
    materialize_verified,
};

/// Embed bytecode with 64-byte alignment (zero-copy loading).
///
/// Use this instead of `include_bytes!` to ensure the embedded bytecode
/// is properly aligned for DFA deserialization and cache efficiency.
///
/// # Example
///
/// ```ignore
/// use plotnik_lib::{include_query_aligned, bytecode::Module};
///
/// let module = Module::from_static(include_query_aligned!("query.ptk.bin"))?;
/// ```
#[macro_export]
macro_rules! include_query_aligned {
    ($path:expr) => {{
        #[repr(C, align(64))]
        struct Aligned<const N: usize>([u8; N]);

        const BYTES: &[u8] = include_bytes!($path);
        static ALIGNED: Aligned<{ BYTES.len() }> = Aligned(*BYTES);
        ALIGNED.0.as_slice()
    }};
}
