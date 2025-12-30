mod dump;
mod invariants;
mod printer;
mod source_map;
mod utils;
pub use printer::QueryPrinter;
pub use query::{Query, QueryBuilder};
pub use source_map::{SourceId, SourceMap};
pub use symbol_table::SymbolTable;

pub mod alt_kinds;
mod dependencies;
pub mod link;
#[allow(clippy::module_inception)]
pub mod query;
pub mod symbol_table;
pub mod type_check;
pub mod visitor;

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod dependencies_tests;
#[cfg(all(test, feature = "plotnik-langs"))]
mod link_tests;
#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod symbol_table_tests;
