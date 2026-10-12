//! Bytecode format constants.

use super::effects::EFFECT_PAYLOAD_BITS;

/// Magic bytes identifying a Plotnik bytecode module.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
///
/// Plotnik is pre-release, so format changes update all in-tree producers and
/// consumers together while this stays zero. The header field and loader check
/// remain in place for the first compatibility-bearing release.
pub const VERSION: u32 = 0;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// Buffer header size in bytes.
///
/// The header occupies exactly one `SECTION_ALIGN` block, so the first section
/// (StringBlob) begins at this offset. `Header` statically asserts it has this
/// size.
pub const HEADER_SIZE: usize = SECTION_ALIGN;

/// Bytecode-word size in bytes. Instructions are word-aligned and may span words.
pub const BYTECODE_WORD_SIZE: usize = 8;

/// String offset table entry size: one little-endian `u32` offset per string.
pub const STRING_TABLE_ENTRY_SIZE: usize = size_of::<u32>();

/// Regex table entry size: `string_id (u16) | reserved (u16) | offset (u32)`.
pub const REGEX_TABLE_ENTRY_SIZE: usize = 8;

/// Spans-section entry size in bytes.
pub const SPAN_ENTRY_SIZE: usize = 16;

/// Hard ceiling on spans per module: span ids live in the effect payload.
pub const MAX_SPANS: usize = 1 << EFFECT_PAYLOAD_BITS;

/// Maximum payload slots for Match instructions.
///
/// Match64 (the largest variant) supports up to 28 u16 slots for
/// effects, neg_fields, and successors combined. When an epsilon
/// instruction needs more successors, it must be split into a cascade.
pub const MAX_MATCH_PAYLOAD_SLOTS: usize = 28;

/// Maximum effects per Match instruction (4-bit count field).
pub const MAX_EFFECTS: usize = 15;

/// Maximum negated fields per Match instruction (3-bit count field).
pub const MAX_NEG_FIELDS: usize = 7;

/// Maximum successors per Match instruction (5-bit count field).
pub const MAX_SUCCESSORS: usize = 31;
