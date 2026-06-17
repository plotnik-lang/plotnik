//! Bytecode format constants.

/// Magic bytes identifying a Plotnik bytecode file.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
/// v2: Removed explicit offsets (computed from counts), added regex section.
/// v3: Removed flags field.
/// v4: Removed the trivia section.
/// v5: Added extras-only anchor navigation modes.
/// v6: Reserved bit 7 of a Nav byte for the Up family (uniform 5-bit level).
pub const VERSION: u32 = 6;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// File header size in bytes.
///
/// The header occupies exactly one `SECTION_ALIGN` block, so the first section
/// (StringBlob) begins at this offset. `Header` statically asserts it has this
/// size.
pub const HEADER_SIZE: usize = SECTION_ALIGN;

/// Step size in bytes (all instructions are 8-byte aligned).
pub const STEP_SIZE: usize = 8;

/// String offset table entry size: one little-endian `u32` offset per string.
pub const STRING_TABLE_ENTRY_SIZE: usize = size_of::<u32>();

/// Regex table entry size: `string_id (u16) | reserved (u16) | offset (u32)`.
pub const REGEX_TABLE_ENTRY_SIZE: usize = 8;

/// Maximum payload slots for Match instructions.
///
/// Match64 (the largest variant) supports up to 28 u16 slots for
/// effects, neg_fields, and successors combined. When an epsilon
/// transition needs more successors, it must be split into a cascade.
pub const MAX_MATCH_PAYLOAD_SLOTS: usize = 28;

/// Maximum pre-effects per Match instruction.
///
/// Pre-effect count is stored in 3 bits (max 7). When exceeded,
/// overflow effects must be emitted in leading epsilon transitions.
pub const MAX_PRE_EFFECTS: usize = 7;

/// Maximum negated fields per Match instruction (3-bit count field).
pub const MAX_NEG_FIELDS: usize = 7;

/// Maximum post-effects per Match instruction (3-bit count field).
pub const MAX_POST_EFFECTS: usize = 7;

/// Maximum successors per Match instruction (5-bit count field).
pub const MAX_SUCCESSORS: usize = 31;
