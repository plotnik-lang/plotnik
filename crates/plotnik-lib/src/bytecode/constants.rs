//! Bytecode format constants.

/// Magic bytes identifying a Plotnik bytecode file.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
pub const VERSION: u32 = 1;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// Step size in bytes (all instructions are 8-byte aligned).
pub const STEP_SIZE: usize = 8;

/// Sentinel value for "any named node" wildcard `(_)`.
///
/// When `node_type` equals this value, the VM checks `node.is_named()`
/// instead of comparing type IDs. This distinguishes `(_)` (any named)
/// from `_` (any node including anonymous).
pub const NAMED_WILDCARD: u16 = 0xFFFF;

/// Maximum payload slots for Match instructions.
///
/// Match64 (the largest variant) supports up to 28 u16 slots for
/// effects, neg_fields, and successors combined. When an epsilon
/// transition needs more successors, it must be split into a cascade.
pub const MAX_MATCH_PAYLOAD_SLOTS: usize = 28;
