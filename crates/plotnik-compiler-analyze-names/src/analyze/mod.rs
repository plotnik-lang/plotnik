pub mod validation {
    pub use plotnik_compiler_core::ValidatedAst;
}

pub mod visitor {
    pub use plotnik_compiler_core::visitor::*;
}

#[path = "../../../plotnik-compiler/src/analyze/symbol_table.rs"]
pub mod symbol_table;

pub use plotnik_compiler_core::Located;
pub use symbol_table::{SymbolTable, resolve_names};
