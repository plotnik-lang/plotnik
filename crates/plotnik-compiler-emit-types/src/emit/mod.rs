pub use plotnik_compiler_emit_strings::{EmitError, StringTableBuilder};

#[path = "../../../plotnik-compiler/src/emit/type_table.rs"]
pub mod type_table;

pub use type_table::TypeTableBuilder;
