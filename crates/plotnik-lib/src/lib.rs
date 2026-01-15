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
//! let query = Query::try_from(source).expect("out of fuel");
//! eprintln!("{}", query.diagnostics().render(query.source_map()));
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

// Re-export modules from plotnik-core
pub use plotnik_core::colors;

// Re-export modules from plotnik-bytecode
pub use plotnik_bytecode::type_system;
pub use plotnik_bytecode as bytecode;

// Re-export modules from plotnik-compiler
pub use plotnik_compiler::analyze;
pub use plotnik_compiler::compile;
pub use plotnik_compiler::diagnostics;
pub use plotnik_compiler::emit;
pub use plotnik_compiler::parser;
pub use plotnik_compiler::query;
pub use plotnik_compiler::typegen;

// Re-export modules from plotnik-vm
pub use plotnik_vm::engine;

// Re-export key types from core
pub use plotnik_core::Colors;

// Re-export key types from compiler
pub use plotnik_compiler::{
    Diagnostics, DiagnosticsPrinter, Error, PassResult, Result, Severity, Span,
};
pub use plotnik_compiler::{Query, QueryBuilder, SourceId, SourceMap};

// Re-export VM types
pub use plotnik_vm::{
    EffectLog, FuelLimits, Materializer, NodeHandle, PrintTracer, RuntimeEffect, RuntimeError,
    Tracer, Value, ValueMaterializer, Verbosity, VM, debug_verify_type,
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
