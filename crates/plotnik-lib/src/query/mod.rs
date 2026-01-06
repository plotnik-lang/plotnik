//! Query facade for Plotnik compilation.

mod dump;
mod printer;
mod source_map;
mod stages;

#[cfg(test)]
mod printer_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod source_map_tests;

// Public API
pub use printer::QueryPrinter;
pub use source_map::{Source, SourceId, SourceKind, SourceMap};
pub use stages::{
    AstMap, LinkedQuery, Query, QueryAnalyzed, QueryBuilder, QueryConfig, QueryContext, QueryParsed,
};
