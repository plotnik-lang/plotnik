//! Plotnik: Query language for tree-sitter AST with type inference.
//!
//! # Example
//!
//! ```
//! use plotnik_lib::{Query, RenderOptions};
//!
//! let query = Query::new(r#"
//!     Expr = [(identifier) (number)]
//!     (assignment left: (Expr) @lhs right: (Expr) @rhs)
//! "#);
//!
//! if !query.is_valid() {
//!     eprintln!("{}", query.render_diagnostics(RenderOptions::plain()));
//! }
//! ```

pub mod ast;
pub mod query;

pub use ast::{Diagnostic, RenderOptions, Severity};
pub use query::Query;
