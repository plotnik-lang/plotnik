#[path = "../../../plotnik-compiler/src/analyze/dependencies.rs"]
pub mod dependencies;
#[path = "../../../plotnik-compiler/src/analyze/recursion.rs"]
mod recursion;
#[path = "../../../plotnik-compiler/src/analyze/refs.rs"]
pub mod refs;

pub mod symbol_table {
    pub use plotnik_compiler_analyze_names::symbol_table::*;
}

pub mod type_check {
    pub use plotnik_compiler_core::DefId;
}

pub mod visitor {
    pub use plotnik_compiler_analyze_shape::visitor::*;
}

pub use dependencies::{DependencyAnalysis, analyze_dependencies};
pub use plotnik_compiler_analyze_shape::Located;
pub use recursion::validate_recursion;
