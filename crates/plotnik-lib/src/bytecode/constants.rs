//! Bytecode format constants.

// Re-export primitive type constants from the shared type system
pub use crate::type_system::{TYPE_CUSTOM_START, TYPE_NODE, TYPE_STRING, TYPE_VOID};

/// Magic bytes identifying a Plotnik bytecode file.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
pub const VERSION: u32 = 1;

/// Terminal step - accept state.
pub const STEP_ACCEPT: u16 = 0;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// Step size in bytes (all instructions are 8-byte aligned).
pub const STEP_SIZE: usize = 8;
