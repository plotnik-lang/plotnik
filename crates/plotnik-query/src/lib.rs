#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub use plotnik_compiler_analyze::analyze::*;
}

pub mod bytecode {
    pub use plotnik_compiler_ir::*;
}

pub mod compile {
    pub use plotnik_compiler_lower::compile::*;
}

pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}

pub mod emit {
    pub use plotnik_compiler_emit::emit::*;
}

pub mod parser {
    pub use plotnik_compiler_parse::parser::*;
}

pub mod source {
    pub use plotnik_compiler_diagnostics::source::*;
}

pub use plotnik_compiler_diagnostics::{Diagnostics, Error, Result, SourceId, SourceMap};

pub mod query {
    #[path = "../../../plotnik-compiler/src/query/dump.rs"]
    mod dump;
    #[path = "../../../plotnik-compiler/src/query/printer.rs"]
    mod printer;
    #[path = "../../../plotnik-compiler/src/query/stages.rs"]
    mod stages;

    pub use crate::source::{Source, SourceId, SourceKind, SourceMap};
    pub use printer::QueryPrinter;
    pub use stages::{AstMap, GrammarBoundQuery, Query, QueryBuilder, QueryConfig, QueryParsed};
}

pub use query::*;
