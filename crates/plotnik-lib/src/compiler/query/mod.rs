//! Query facade for Plotnik compilation.

mod dump;
mod printer;
mod stages;

#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod stages_tests;

pub(crate) use stages::LinkOutcome;
pub use stages::{CompiledQuery, Query, QueryBuilder};
