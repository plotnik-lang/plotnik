use super::effects::EFFECT_PAYLOAD_BITS;

pub const MAGIC: [u8; 4] = *b"PTKQ";

pub const VERSION: u32 = 0;

pub const SECTION_ALIGN: usize = 64;

pub const HEADER_SIZE: usize = SECTION_ALIGN;

pub const BYTECODE_WORD_SIZE: usize = 8;

pub const STRING_TABLE_ENTRY_SIZE: usize = size_of::<u32>();

pub const REGEX_TABLE_ENTRY_SIZE: usize = 8;

pub const SPAN_ENTRY_SIZE: usize = 16;

pub const MAX_SPANS: usize = 1 << EFFECT_PAYLOAD_BITS;

pub const MAX_MATCH_PAYLOAD_SLOTS: usize = 28;

pub const MAX_EFFECTS: usize = 15;

pub const MAX_NEG_FIELDS: usize = 7;

pub const MAX_SUCCESSORS: usize = 31;
