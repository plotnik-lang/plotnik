#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub mod symbol_table {
        pub use plotnik_compiler_core::SymbolTable;
    }

    pub mod type_check {
        pub use plotnik_compiler_core::{
            CaptureMechanism, DefId, TypeAnalysis, TypeId, TypeShape, classify_capture_mechanism,
            ref_returns_structured,
        };
    }
}

pub mod bytecode {
    pub use plotnik_compiler_core::ir::*;
}

pub mod parser {
    pub use plotnik_compiler_core::ast;
    pub use plotnik_compiler_core::{Pattern, Ref, SeqItem, SyntaxKind};
}

pub mod compile;
pub use compile::*;
