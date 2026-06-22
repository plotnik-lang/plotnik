#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod analyze {
    pub mod symbol_table {
        pub use plotnik_compiler_analyze_names::symbol_table::*;
    }

    pub mod type_check {
        pub use plotnik_compiler_analyze_types::type_check::*;
    }
}

pub mod bytecode {
    pub use plotnik_compiler_core::ir::*;
}

pub mod parser {
    pub use plotnik_compiler_parse::parser::*;
}

pub mod compile;
pub use compile::*;
