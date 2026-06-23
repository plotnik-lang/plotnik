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

pub mod bytecode;
mod compiler;
mod core;
mod vm;

pub use crate::bytecode::type_system;
pub use crate::core::colors;
pub use crate::core::grammar;

pub mod diagnostics {
    pub use crate::compiler::diagnostics::diagnostics::*;
    pub use crate::compiler::diagnostics::*;
}

#[doc(hidden)]
pub mod parser {
    pub use crate::compiler::parse::dump_tokens;
}

pub mod typegen {
    pub use crate::compiler::typegen::*;
}

pub use crate::core::Colors;

pub use crate::compiler::query::QueryPrinter;
pub use crate::compiler::{Diagnostics, EmitError, Error, PassResult, Result, Severity, Span};
pub use crate::compiler::{
    GrammarBoundQuery, Query, QueryBuilder, QueryParsed, SourceId, SourceMap,
};

pub use crate::vm::{
    EffectLog, ExecLimits, Materializer, NodeHandle, PrintTracer, RuntimeEffect, RuntimeError,
    Tracer, VM, Value, ValueMaterializer, Verbosity, debug_verify_type, materialize_verified,
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
