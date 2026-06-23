//! Plotnik compiler compatibility facade.
//!
//! The pipeline stages live in separate crates. This crate preserves the
//! historical `plotnik_compiler::{parser, analyze, compile, emit, query, ...}`
//! public surface for downstream callers.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod emit;
pub mod query;
#[cfg(test)]
pub mod test_utils;

#[cfg(test)]
mod analyze_tests;
#[cfg(test)]
mod parser_tests;

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
pub mod compile {
    pub use plotnik_compiler_lower_dead::remove_unreachable;
    pub use plotnik_compiler_lower_epsilon::eliminate_epsilons;
    pub use plotnik_compiler_lower_nav::collapse_up;
    pub use plotnik_compiler_lower_pack::lower;
    pub use plotnik_compiler_lower_thompson::{CaptureEffects, CompileCtx, CompileResult, Compiler};

    pub mod verify {
        pub use plotnik_compiler_lower_thompson::verify::*;
    }

    #[cfg(test)]
    mod compile_tests;
}
pub mod diagnostics {
    pub use plotnik_compiler_diagnostics::*;
    pub use plotnik_compiler_diagnostics::diagnostics::*;
}
pub use plotnik_compiler_diagnostics::source;
pub use plotnik_compiler_core::ir as bytecode;
pub use plotnik_compiler_parse as parser;
pub use plotnik_compiler_typegen as typegen;

pub use plotnik_compiler_diagnostics::{
    Diagnostics, Error, PassResult, Result, Severity, SourceId, SourceMap, Span,
};
pub use query::{Query, QueryBuilder};
