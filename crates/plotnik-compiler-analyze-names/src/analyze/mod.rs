pub mod validation {
    pub use plotnik_compiler_analyze_shape::validation::*;
}

pub mod visitor {
    pub use plotnik_compiler_analyze_shape::visitor::*;
}

#[path = "../../../plotnik-compiler/src/analyze/symbol_table.rs"]
pub mod symbol_table;

pub use plotnik_compiler_analyze_shape::Located;
pub use symbol_table::{SymbolTable, UNNAMED_DEF, resolve_names};
