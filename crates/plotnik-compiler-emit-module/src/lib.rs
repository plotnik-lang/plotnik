#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub mod type_check {
        pub use plotnik_compiler_core::TypeAnalysis;
    }
}

pub mod bytecode {
    pub use plotnik_compiler_core::ir::*;
}

pub mod emit;
pub use emit::*;
