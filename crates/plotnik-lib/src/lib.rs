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
//! "#);
//!
//! if !query.is_valid() {
//!     eprintln!("{}", query.dump_errors());
//! }
//! ```

pub mod ast;
pub mod query;

pub use query::Query;
