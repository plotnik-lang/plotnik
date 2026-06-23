//! Shared bytecode-emission data types.
//!
//! The emit pipeline lives in the per-phase `compiler::emit` modules;
//! this module owns the data the phases produce and read across crate
//! boundaries: the error type and the string, type, and regex tables. The
//! tables carry their construction *state* and serialization, but no algorithm
//! that walks the IR and no regex engine — those belong to the phase modules.

mod context;
mod error;
mod regex_table;
mod string_table;
mod type_table;

#[cfg(test)]
mod string_table_tests;
#[cfg(test)]
mod type_table_tests;

pub use context::EmitInput;
pub use error::EmitError;
pub use regex_table::RegexTableBuilder;
pub use string_table::{EASTER_EGG, StringTableBuilder};
pub use type_table::TypeTableBuilder;
