#[path = "../../../plotnik-compiler/src/emit/error.rs"]
pub mod error;
#[path = "../../../plotnik-compiler/src/emit/string_table.rs"]
pub mod string_table;

pub use error::EmitError;
pub use string_table::{EASTER_EGG, StringTableBuilder};
