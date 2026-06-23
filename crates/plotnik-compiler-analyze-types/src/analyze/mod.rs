#[path = "../../../plotnik-compiler/src/analyze/entrypoints.rs"]
mod entrypoints;
#[path = "../../../plotnik-compiler/src/analyze/type_check/mod.rs"]
pub mod type_check;

pub mod dependencies {
    pub use plotnik_compiler_core::DependencyAnalysis;
}

pub mod symbol_table {
    pub use plotnik_compiler_core::SymbolTable;
}

pub use entrypoints::validate_entrypoints;
pub use plotnik_compiler_core::Located;
pub use type_check::{TypeAnalysis, TypeAnalysisBuilder, infer_types};
