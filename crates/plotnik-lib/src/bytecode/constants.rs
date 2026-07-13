//! Bytecode format constants.

use super::effects::EFFECT_PAYLOAD_BITS;

/// Magic bytes identifying a Plotnik bytecode module.
pub const MAGIC: [u8; 4] = *b"PTKQ";

/// Current bytecode format version.
/// v2: Removed explicit offsets (computed from counts), added regex section.
/// v3: Removed flags field.
/// v4: Removed the trivia section.
/// v5: Added extras-only anchor navigation modes.
/// v6: Reserved bit 7 of a Nav byte for the Up family (uniform 5-bit level).
/// v7: Type kind and effect opcode discriminants renumbered contiguously.
/// v8: single effects list per Match; per-entrypoint wrappers.
/// v9: `Childless*` navigation family (anchors over a zero-width child list).
/// v10: inspection spans — three span effect kinds and the `spans` section.
/// v11: `Text`/`Bool` types and balanced scalar-provenance effects.
pub const VERSION: u32 = 11;

/// Section alignment in bytes.
pub const SECTION_ALIGN: usize = 64;

/// Buffer header size in bytes.
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

/// Spans-section entry size in bytes.
pub const SPAN_ENTRY_SIZE: usize = 16;

/// Hard ceiling on spans per module: span ids live in the effect payload.
pub const MAX_SPANS: usize = 1 << EFFECT_PAYLOAD_BITS;

/// Maximum payload slots for Match instructions.
///
/// Match64 (the largest variant) supports up to 28 u16 slots for
/// effects, neg_fields, and successors combined. When an epsilon
/// transition needs more successors, it must be split into a cascade.
pub const MAX_MATCH_PAYLOAD_SLOTS: usize = 28;

/// Maximum effects per Match instruction (4-bit count field).
pub const MAX_EFFECTS: usize = 15;

/// Maximum negated fields per Match instruction (3-bit count field).
pub const MAX_NEG_FIELDS: usize = 7;

/// Maximum successors per Match instruction (5-bit count field).
pub const MAX_SUCCESSORS: usize = 31;
