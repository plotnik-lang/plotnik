//! Plotnik compiler compatibility facade.
//!
//! The pipeline stages live in separate crates. This crate preserves the
//! historical `plotnik_compiler::{parser, analyze, compile, emit, query, ...}`
//! public surface for downstream callers.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg(test)]
pub mod analyze;
#[cfg(test)]
pub mod bytecode;
#[cfg(test)]
pub mod compile;
#[cfg(test)]
pub mod diagnostics;
pub mod emit;
#[cfg(test)]
pub mod parser;
pub mod query;
#[cfg(test)]
pub mod source {
    pub use plotnik_compiler_diagnostics::source::*;
}
#[cfg(test)]
pub mod test_utils;

#[cfg(not(test))]
pub mod analyze {
    pub mod dependencies {
        pub use plotnik_compiler_analyze_refs::dependencies::*;
    }

    pub mod link {
        pub use plotnik_compiler_analyze_grammar::link::*;
    }

    pub mod refs {
        pub use plotnik_compiler_analyze_refs::refs::*;
    }

    pub mod symbol_table {
        pub use plotnik_compiler_analyze_names::symbol_table::*;
    }

    pub mod type_check {
        pub use plotnik_compiler_analyze_types::type_check::*;
    }

    pub mod validation {
        pub use plotnik_compiler_analyze_shape::validation::*;
    }

    pub mod visitor {
        pub use plotnik_compiler_core::visitor::*;
    }

    pub use dependencies::{DependencyAnalysis, analyze_dependencies};
    pub use link::GrammarBinding;
    pub use plotnik_compiler_analyze_grammar::GrammarLinkCtx;
    pub use plotnik_compiler_analyze_names::{SymbolTable, resolve_names};
    pub use plotnik_compiler_analyze_refs::validate_recursion;
    pub use plotnik_compiler_core::Located;
    pub use plotnik_compiler_analyze_shape::validation::{
        validate_alt_kinds, validate_anchors, validate_empty_constructs, validate_predicates,
    };
    pub use plotnik_compiler_analyze_types::{
        TypeAnalysis, TypeAnalysisBuilder, infer_types, validate_entrypoints,
    };
}
#[cfg(not(test))]
pub mod compile {
    pub use plotnik_compiler_lower_dead::remove_unreachable;
    pub use plotnik_compiler_lower_epsilon::eliminate_epsilons;
    pub use plotnik_compiler_lower_nav::collapse_up;
    pub use plotnik_compiler_lower_pack::lower;
    pub use plotnik_compiler_lower_thompson::{CaptureEffects, CompileCtx, CompileResult, Compiler};

    pub mod verify {
        pub use plotnik_compiler_lower_thompson::compile::verify::*;
    }
}
#[cfg(not(test))]
pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::*;
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}
#[cfg(not(test))]
pub use plotnik_compiler_diagnostics::source;
#[cfg(not(test))]
pub use plotnik_compiler_core::ir as bytecode;
#[cfg(not(test))]
pub use plotnik_compiler_parse as parser;
pub use plotnik_compiler_typegen as typegen;

#[cfg(test)]
pub type PassResult<T> = std::result::Result<(T, Diagnostics), Error>;

#[cfg(test)]
pub use diagnostics::{Diagnostics, Severity, Span};
#[cfg(test)]
pub use query::{Query, QueryBuilder};
#[cfg(test)]
pub use source::{SourceId, SourceMap};

#[cfg(test)]
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    /// Execution fuel exhausted (too many parser operations).
    #[error("execution limit exceeded")]
    ParseFuelExhausted,

    /// Recursion fuel exhausted (input nested too deeply).
    #[error("recursion limit exceeded")]
    RecursionLimitExceeded,

    #[error("query parsing failed with {} errors", .0.error_count())]
    QueryParseError(Diagnostics),

    #[error("query analysis failed with {} errors", .0.error_count())]
    QueryAnalyzeError(Diagnostics),
}

#[cfg(test)]
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(not(test))]
pub use plotnik_compiler_diagnostics::{Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span};
#[cfg(not(test))]
pub use query::{Query, QueryBuilder};
