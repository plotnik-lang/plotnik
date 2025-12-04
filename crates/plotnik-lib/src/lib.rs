//! Plotnik: Query language for tree-sitter AST with type inference.
//!
//! # Example
//!
//! ```
//! use plotnik_lib::Query;
//!
//! let query = Query::new(r#"
//!     Expr = [(identifier) (number)]
//!     (assignment left: (Expr) @lhs right: (Expr) @rhs)
//! "#).expect("valid query");
//!
//! if !query.is_valid() {
//!     eprintln!("{}", query.diagnostics_printer().render());
//! }
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics;
pub mod parser;
pub mod query;

pub use diagnostics::{
    DiagnosticMessage, DiagnosticStage, Diagnostics, DiagnosticsPrinter, Fix, RelatedInfo, Severity,
};
pub use query::{Query, QueryBuilder};

/// Errors that can occur during query parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Execution fuel exhausted (too many parser operations).
    #[error("execution limit exceeded")]
    ExecFuelExhausted,

    /// Recursion fuel exhausted (input nested too deeply).
    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, Error>;
