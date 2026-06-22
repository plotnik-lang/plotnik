#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod bytecode {
    pub use plotnik_compiler_ir::*;
}

pub mod compile;
pub use compile::*;
