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
//! eprintln!("{}", query.diagnostics().render(source));
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics;
pub mod engine;
pub mod ir;
pub mod parser;
pub mod query;

/// Result type for analysis passes that produce both output and diagnostics.
///
/// Each pass returns its typed output alongside any diagnostics it collected.
/// Fatal errors (like fuel exhaustion) use the outer `Result`.
pub type PassResult<T> = std::result::Result<(T, Diagnostics), Error>;

pub use diagnostics::{Diagnostics, DiagnosticsPrinter, Severity};
pub use query::{Query, UNNAMED_DEF};

/// Errors that can occur during query parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Execution fuel exhausted (too many parser operations).
    #[error("execution limit exceeded")]
    ExecFuelExhausted,

    /// Recursion fuel exhausted (input nested too deeply).
    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,

    #[error("query parsing failed with {} errors", .0.error_count())]
    QueryParseError(Diagnostics),

    #[error("query analysis failed with {} errors", .0.error_count())]
    QueryAnalyzeError(Diagnostics),
}

/// Result type for query operations.
pub type Result<T> = std::result::Result<T, Error>;
