//! Bytecode format constants.

/// Magic bytes identifying a Plotnik bytecode file.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
pub const VERSION: u32 = 1;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// Step size in bytes (all instructions are 8-byte aligned).
pub const STEP_SIZE: usize = 8;
