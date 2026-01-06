mod dump;
mod printer;
pub mod source_map;

pub use printer::QueryPrinter;
pub use query::{Query, QueryBuilder, QueryContext};
pub use source_map::{SourceId, SourceMap};

#[allow(clippy::module_inception)]
pub mod query;

// Re-export from analyze/ for backwards compatibility
pub use crate::analyze::SymbolTable;

#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod source_map_tests;
