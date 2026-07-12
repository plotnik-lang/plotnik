//! Bytecode header (64 bytes).
//!
//! Offsets are computed from counts + SECTION_ALIGN (64 bytes); no stored offsets.
//! Section order: Header → StringBlob → RegexBlob → StringTable → RegexTable →
//! NodeKinds → NodeFields → TypeDefs → TypeMembers → TypeNames → Entrypoints →
//! Transitions → Spans

use super::entrypoint::Entrypoint;
use super::sections::SymbolNameEntry;
use super::type_meta::{TypeDef, TypeMember, TypeNameEntry};
use super::{
    HEADER_SIZE, MAGIC, REGEX_TABLE_ENTRY_SIZE, SECTION_ALIGN, SPAN_ENTRY_SIZE, STEP_SIZE,
    STRING_TABLE_ENTRY_SIZE, VERSION,
};

/// Number of sections after the header, in layout order. The single descriptor
/// of that layout is [`Header::section_data_sizes`].
pub(crate) const SECTION_COUNT: usize = 12;

/// First 64 bytes of the bytecode buffer.
///
/// Layout (offsets computed from counts):
/// - 0-23: identity and sizes (magic, version, checksum, total_size, str_blob_size, regex_blob_size)
/// - 24-41: counts (9 × u16) — order matches section order
/// - 42-43: spans_count
/// - 44-63: reserved
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Header {
    // Bytes 0-23: Identity and sizes (6 × u32)
    /// Magic bytes: b"PTKQ"
    pub magic: [u8; 4],
    /// Bytecode format version.
    pub version: u32,
    /// CRC32 checksum of everything after the header
    pub checksum: u32,
    /// Total buffer size in bytes.
    pub total_size: u32,
    /// Size of the string blob in bytes.
    pub str_blob_size: u32,
    /// Size of the regex blob in bytes.
    pub regex_blob_size: u32,

    // Bytes 24-41: Element counts (9 × u16) — order matches section order
    pub str_table_count: u16,
    pub regex_table_count: u16,
    pub node_kinds_count: u16,
    pub node_fields_count: u16,
    pub type_defs_count: u16,
    pub type_members_count: u16,
    pub type_names_count: u16,
    pub entrypoints_count: u16,
    pub transitions_count: u16,

    // Bytes 42-43: Spans section count.
    pub spans_count: u16,

    // Bytes 44-63: Reserved (public for cross-crate struct initialization)
    pub _reserved: [u8; 20],
}

const _: () = assert!(std::mem::size_of::<Header>() == HEADER_SIZE);

impl Default for Header {
    fn default() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            checksum: 0,
            total_size: 0,
            str_blob_size: 0,
            regex_blob_size: 0,
            str_table_count: 0,
            regex_table_count: 0,
            node_kinds_count: 0,
            node_fields_count: 0,
            type_defs_count: 0,
            type_members_count: 0,
            type_names_count: 0,
            entrypoints_count: 0,
            transitions_count: 0,
            spans_count: 0,
            _reserved: [0; 20],
        }
    }
}

/// Computed section offsets derived from header counts.
///
/// Order: StringBlob → RegexBlob → StringTable → RegexTable → NodeKinds →
/// NodeFields → TypeDefs → TypeMembers → TypeNames → Entrypoints → Transitions → Spans
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionOffsets {
    pub(crate) str_blob: u32,
    pub(crate) regex_blob: u32,
    pub(crate) str_table: u32,
    pub(crate) regex_table: u32,
    pub(crate) node_kinds: u32,
    pub(crate) node_fields: u32,
    pub(crate) type_defs: u32,
    pub(crate) type_members: u32,
    pub(crate) type_names: u32,
    pub(crate) entrypoints: u32,
    pub(crate) transitions: u32,
    pub(crate) spans: u32,
}

impl SectionOffsets {
    /// Build from section start offsets in layout order.
    fn from_starts(o: [u32; SECTION_COUNT]) -> Self {
        Self {
            str_blob: o[0],
            regex_blob: o[1],
            str_table: o[2],
            regex_table: o[3],
            node_kinds: o[4],
            node_fields: o[5],
            type_defs: o[6],
            type_members: o[7],
            type_names: o[8],
            entrypoints: o[9],
            transitions: o[10],
            spans: o[11],
        }
    }

    /// Section start offsets in layout order.
    pub(crate) fn as_starts(&self) -> [u32; SECTION_COUNT] {
        [
            self.str_blob,
            self.regex_blob,
            self.str_table,
            self.regex_table,
            self.node_kinds,
            self.node_fields,
            self.type_defs,
            self.type_members,
            self.type_names,
            self.entrypoints,
            self.transitions,
            self.spans,
        ]
    }
}

impl Header {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= HEADER_SIZE, "header too short");

        let mut reserved = [0u8; 20];
        reserved.copy_from_slice(&bytes[44..HEADER_SIZE]);

        Self {
            magic: [bytes[0], bytes[1], bytes[2], bytes[3]],
            version: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            checksum: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            total_size: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            str_blob_size: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            regex_blob_size: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            str_table_count: u16::from_le_bytes([bytes[24], bytes[25]]),
            regex_table_count: u16::from_le_bytes([bytes[26], bytes[27]]),
            node_kinds_count: u16::from_le_bytes([bytes[28], bytes[29]]),
            node_fields_count: u16::from_le_bytes([bytes[30], bytes[31]]),
            type_defs_count: u16::from_le_bytes([bytes[32], bytes[33]]),
            type_members_count: u16::from_le_bytes([bytes[34], bytes[35]]),
            type_names_count: u16::from_le_bytes([bytes[36], bytes[37]]),
            entrypoints_count: u16::from_le_bytes([bytes[38], bytes[39]]),
            transitions_count: u16::from_le_bytes([bytes[40], bytes[41]]),
            spans_count: u16::from_le_bytes([bytes[42], bytes[43]]),
            _reserved: reserved,
        }
    }

    pub fn to_bytes(self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.total_size.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.str_blob_size.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.regex_blob_size.to_le_bytes());
        bytes[24..26].copy_from_slice(&self.str_table_count.to_le_bytes());
        bytes[26..28].copy_from_slice(&self.regex_table_count.to_le_bytes());
        bytes[28..30].copy_from_slice(&self.node_kinds_count.to_le_bytes());
        bytes[30..32].copy_from_slice(&self.node_fields_count.to_le_bytes());
        bytes[32..34].copy_from_slice(&self.type_defs_count.to_le_bytes());
        bytes[34..36].copy_from_slice(&self.type_members_count.to_le_bytes());
        bytes[36..38].copy_from_slice(&self.type_names_count.to_le_bytes());
        bytes[38..40].copy_from_slice(&self.entrypoints_count.to_le_bytes());
        bytes[40..42].copy_from_slice(&self.transitions_count.to_le_bytes());
        bytes[42..44].copy_from_slice(&self.spans_count.to_le_bytes());
        bytes[44..HEADER_SIZE].copy_from_slice(&self._reserved);
        bytes
    }

    pub fn has_valid_magic(&self) -> bool {
        self.magic == MAGIC
    }

    pub fn is_supported_version(&self) -> bool {
        self.version == VERSION
    }

    /// Data size (bytes, before alignment padding) of each section, in layout
    /// order. This is the single descriptor of the section layout — offset
    /// computation ([`compute_offsets`](Self::compute_offsets)) and the load-time
    /// bounds/padding checks all fold over it.
    ///
    /// Widened to `u64` so a corrupt header cannot overflow a running layout.
    ///
    /// Order: StringBlob → RegexBlob → StringTable → RegexTable → NodeKinds →
    /// NodeFields → TypeDefs → TypeMembers → TypeNames → Entrypoints →
    /// Transitions → Spans
    pub(crate) fn section_data_sizes(&self) -> [u64; SECTION_COUNT] {
        // Tables carry a trailing sentinel entry, hence the `+ 1`.
        [
            self.str_blob_size as u64,
            self.regex_blob_size as u64,
            (self.str_table_count as u64 + 1) * STRING_TABLE_ENTRY_SIZE as u64,
            (self.regex_table_count as u64 + 1) * REGEX_TABLE_ENTRY_SIZE as u64,
            self.node_kinds_count as u64 * SymbolNameEntry::SIZE as u64,
            self.node_fields_count as u64 * SymbolNameEntry::SIZE as u64,
            self.type_defs_count as u64 * TypeDef::SIZE as u64,
            self.type_members_count as u64 * TypeMember::SIZE as u64,
            self.type_names_count as u64 * TypeNameEntry::SIZE as u64,
            self.entrypoints_count as u64 * Entrypoint::SIZE as u64,
            self.transitions_count as u64 * STEP_SIZE as u64,
            self.spans_count as u64 * SPAN_ENTRY_SIZE as u64,
        ]
    }

    /// Compute section start offsets from counts and blob sizes.
    ///
    /// Each section is `SECTION_ALIGN`-aligned and begins right after the header.
    /// Callers run `Module::validate_section_bounds` first, so the `u32`
    /// arithmetic here cannot overflow.
    pub fn compute_offsets(&self) -> SectionOffsets {
        let align = SECTION_ALIGN as u32;
        let mut starts = [0u32; SECTION_COUNT];
        let mut cursor = HEADER_SIZE as u32; // sections begin right after the header
        for (start, size) in starts.iter_mut().zip(self.section_data_sizes()) {
            *start = cursor;
            cursor = align_up(cursor + size as u32, align);
        }
        SectionOffsets::from_starts(starts)
    }
}

/// Round up to the next multiple of `align`.
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}
