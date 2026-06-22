pub use plotnik_compiler_emit_strings::EmitError;

#[path = "../../../plotnik-compiler/src/emit/regex_table.rs"]
pub mod regex_table;

pub use regex_table::{RegexTableBuilder, deserialize_dfa};
