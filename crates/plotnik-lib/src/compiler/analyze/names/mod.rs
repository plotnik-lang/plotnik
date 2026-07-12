//! Name resolution: build the symbol table from the validated AST.

pub mod resolve;
pub mod symbol_table;

pub use resolve::resolve_names;
pub use symbol_table::SymbolTable;
