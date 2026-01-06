//! Semantic analysis passes.
//!
//! Provides analysis passes that transform parsed AST into analyzed form:
//! - Name resolution (symbol_table)
//! - Dependency analysis and recursion detection (dependencies)
//! - Type inference (type_check)
//! - Grammar linking (link)
//! - Semantic validation (validation)

pub mod dependencies;
mod invariants;
pub mod link;
mod recursion;
pub mod refs;
pub mod symbol_table;
pub mod type_check;
mod utils;
pub mod validation;
pub mod visitor;

#[cfg(test)]
mod dependencies_tests;
#[cfg(all(test, feature = "plotnik-langs"))]
mod link_tests;
#[cfg(test)]
mod refs_tests;
#[cfg(test)]
mod symbol_table_tests;

pub use dependencies::DependencyAnalysis;
pub use link::LinkOutput;
pub use recursion::validate_recursion;
pub use symbol_table::{SymbolTable, UNNAMED_DEF};
pub use type_check::{TypeContext, infer_types, primary_def_name};
pub use validation::{validate_alt_kinds, validate_anchors, validate_empty_constructs};
pub use visitor::{Visitor, walk_expr};
