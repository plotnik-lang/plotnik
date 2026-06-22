#[path = "../../../plotnik-compiler/src/analyze/entrypoints.rs"]
mod entrypoints;
#[path = "../../../plotnik-compiler/src/analyze/type_check/mod.rs"]
pub mod type_check;

pub mod dependencies {
    pub use plotnik_compiler_analyze_refs::dependencies::*;
}

pub mod symbol_table {
    pub use plotnik_compiler_analyze_names::symbol_table::*;
}

pub use entrypoints::validate_entrypoints;
pub use plotnik_compiler_analyze_shape::Located;
pub use type_check::{TypeContext, infer_types};
