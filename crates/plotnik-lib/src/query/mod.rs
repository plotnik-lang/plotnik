mod dump;
mod invariants;
mod printer;
mod utils;
pub use printer::QueryPrinter;
pub use query::{Query, QueryBuilder};

pub mod alt_kinds;
mod dependencies;
pub mod expr_arity;
pub mod link;
#[allow(clippy::module_inception)]
pub mod query;
pub mod symbol_table;
pub mod visitor;

#[cfg(test)]
mod alt_kinds_tests;
#[cfg(test)]
mod dependencies_tests;
#[cfg(test)]
mod expr_arity_tests;
#[cfg(all(test, feature = "plotnik-langs"))]
mod link_tests;
#[cfg(test)]
mod mod_tests;
#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod symbol_table_tests;
