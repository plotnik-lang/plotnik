#[path = "../../../plotnik-compiler/src/analyze/dependencies.rs"]
pub mod dependencies;
#[path = "../../../plotnik-compiler/src/analyze/recursion.rs"]
mod recursion;
#[path = "../../../plotnik-compiler/src/analyze/refs.rs"]
pub mod refs;

pub mod symbol_table {
    pub use plotnik_compiler_core::SymbolTable;
}

pub mod type_check {
    pub use plotnik_compiler_core::DefId;
}

pub mod visitor {
    pub use plotnik_compiler_core::visitor::*;
}

pub use dependencies::{DependencyAnalysis, analyze_dependencies};
pub use plotnik_compiler_core::Located;
pub use recursion::validate_recursion;
