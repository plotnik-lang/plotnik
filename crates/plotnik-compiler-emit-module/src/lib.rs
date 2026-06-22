#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub mod type_check {
        pub use plotnik_compiler_analyze_types::type_check::*;
    }
}

pub mod bytecode {
    pub use plotnik_compiler_ir::*;
}

pub mod emit;
pub use emit::*;
