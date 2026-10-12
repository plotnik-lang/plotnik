//! Shared bytecode-emission data types.
//!
//! The emit phases each live in a sibling `compiler::emit` module; this one owns
//! the data they produce and thread across phase boundaries: the emit error plus
//! the string, type, and regex tables. The tables carry their
//! construction *state* and serialization, but no algorithm that walks the IR
//! and no regex engine — those belong to the phase modules. Each table is a
//! cross-phase accumulator (no single phase owns its full lifecycle), so it
//! lives here at the emit root rather than inside one phase.

mod constant_pool;
mod error;
mod regex_id;
mod regex_table;
mod string_table;
mod type_table;

#[cfg(test)]
mod error_tests;

pub(in crate::compiler::emit) use constant_pool::ConstantPool;
pub(in crate::compiler) use error::EmitError;
pub(in crate::compiler) use regex_id::RegexId;
pub use regex_table::RegexTableBuilder;
pub use string_table::StringTableBuilder;
pub use type_table::TypeTableBuilder;
