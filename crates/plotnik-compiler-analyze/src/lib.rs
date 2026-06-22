#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}

pub mod parser {
    pub use plotnik_compiler_parse::parser::*;
}

pub mod source {
    pub use plotnik_compiler_diagnostics::source::*;
}

pub use plotnik_compiler_diagnostics::{Diagnostics, SourceId};

pub mod analyze {
    #[path = "../../../plotnik-compiler/src/analyze/dependencies.rs"]
    pub mod dependencies;
    #[path = "../../../plotnik-compiler/src/analyze/entrypoints.rs"]
    mod entrypoints;
    #[path = "../../../plotnik-compiler/src/analyze/invariants.rs"]
    mod invariants;
    #[path = "../../../plotnik-compiler/src/analyze/link.rs"]
    pub mod link;
    #[path = "../../../plotnik-compiler/src/analyze/located.rs"]
    mod located;
    #[path = "../../../plotnik-compiler/src/analyze/recursion.rs"]
    mod recursion;
    #[path = "../../../plotnik-compiler/src/analyze/refs.rs"]
    pub mod refs;
    #[path = "../../../plotnik-compiler/src/analyze/symbol_table.rs"]
    pub mod symbol_table;
    #[path = "../../../plotnik-compiler/src/analyze/type_check/mod.rs"]
    pub mod type_check;
    #[path = "../../../plotnik-compiler/src/analyze/utils.rs"]
    mod utils;
    #[path = "../../../plotnik-compiler/src/analyze/validation/mod.rs"]
    pub mod validation;
    #[path = "../../../plotnik-compiler/src/analyze/visitor.rs"]
    pub mod visitor;

    pub use dependencies::DependencyAnalysis;
    pub use entrypoints::validate_entrypoints;
    pub use link::GrammarBinding;
    pub(crate) use located::Located;
    pub use recursion::validate_recursion;
}

pub use analyze::*;
