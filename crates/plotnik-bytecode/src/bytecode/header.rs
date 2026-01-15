//! Bytecode file header (64 bytes).
//!
//! v3 layout: Offsets are computed from counts + SECTION_ALIGN (64 bytes).
//! Section order: Header → StringBlob → RegexBlob → StringTable → RegexTable →
//! NodeTypes → NodeFields → Trivia → TypeDefs → TypeMembers → TypeNames →
//! Entrypoints → Transitions

use super::{MAGIC, SECTION_ALIGN, VERSION};

/// File header - first 64 bytes of the bytecode file.
///
/// v3 layout (offsets computed from counts):
/// - 0-23: identity and sizes (magic, version, checksum, total_size, str_blob_size, regex_blob_size)
/// - 24-43: counts (10 × u16) — order matches section order
/// - 44-63: reserved
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Header {
    // Bytes 0-23: Identity and sizes (6 × u32)
    /// Magic bytes: b"PTKQ"
    pub magic: [u8; 4],
    /// Format version (currently 3)
    pub version: u32,
    /// CRC32 checksum of everything after the header
    pub checksum: u32,
    /// Total file size in bytes
    pub total_size: u32,
    /// Size of the string blob in bytes.
    pub str_blob_size: u32,
    /// Size of the regex blob in bytes.
    pub regex_blob_size: u32,

    // Bytes 24-43: Element counts (10 × u16) — order matches section order
    pub str_table_count: u16,
    pub regex_table_count: u16,
    pub node_types_count: u16,
    pub node_fields_count: u16,
    pub trivia_count: u16,
    pub type_defs_count: u16,
    pub type_members_count: u16,
    pub type_names_count: u16,
    pub entrypoints_count: u16,
    pub transitions_count: u16,

    // Bytes 44-63: Reserved (public for cross-crate struct initialization)
    pub _reserved: [u8; 20],
}

const _: () = assert!(std::mem::size_of::<Header>() == 64);

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
            node_types_count: 0,
            node_fields_count: 0,
            trivia_count: 0,
            type_defs_count: 0,
            type_members_count: 0,
            type_names_count: 0,
            entrypoints_count: 0,
            transitions_count: 0,
            _reserved: [0; 20],
        }
    }
}

/// Computed section offsets derived from header counts.
///
/// Order: StringBlob → RegexBlob → StringTable → RegexTable → NodeTypes →
/// NodeFields → Trivia → TypeDefs → TypeMembers → TypeNames → Entrypoints → Transitions
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SectionOffsets {
    pub str_blob: u32,
    pub regex_blob: u32,
    pub str_table: u32,
    pub regex_table: u32,
    pub node_types: u32,
    pub node_fields: u32,
    pub trivia: u32,
    pub type_defs: u32,
    pub type_members: u32,
    pub type_names: u32,
    pub entrypoints: u32,
    pub transitions: u32,
}

impl Header {
    /// Decode header from 64 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 64, "header too short");

        let mut reserved = [0u8; 20];
        reserved.copy_from_slice(&bytes[44..64]);

        Self {
            magic: [bytes[0], bytes[1], bytes[2], bytes[3]],
            version: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            checksum: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            total_size: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            str_blob_size: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            regex_blob_size: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            str_table_count: u16::from_le_bytes([bytes[24], bytes[25]]),
            regex_table_count: u16::from_le_bytes([bytes[26], bytes[27]]),
            node_types_count: u16::from_le_bytes([bytes[28], bytes[29]]),
            node_fields_count: u16::from_le_bytes([bytes[30], bytes[31]]),
            trivia_count: u16::from_le_bytes([bytes[32], bytes[33]]),
            type_defs_count: u16::from_le_bytes([bytes[34], bytes[35]]),
            type_members_count: u16::from_le_bytes([bytes[36], bytes[37]]),
            type_names_count: u16::from_le_bytes([bytes[38], bytes[39]]),
            entrypoints_count: u16::from_le_bytes([bytes[40], bytes[41]]),
            transitions_count: u16::from_le_bytes([bytes[42], bytes[43]]),
            _reserved: reserved,
        }
    }

    /// Encode header to 64 bytes.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.total_size.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.str_blob_size.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.regex_blob_size.to_le_bytes());
        bytes[24..26].copy_from_slice(&self.str_table_count.to_le_bytes());
        bytes[26..28].copy_from_slice(&self.regex_table_count.to_le_bytes());
        bytes[28..30].copy_from_slice(&self.node_types_count.to_le_bytes());
        bytes[30..32].copy_from_slice(&self.node_fields_count.to_le_bytes());
        bytes[32..34].copy_from_slice(&self.trivia_count.to_le_bytes());
        bytes[34..36].copy_from_slice(&self.type_defs_count.to_le_bytes());
        bytes[36..38].copy_from_slice(&self.type_members_count.to_le_bytes());
        bytes[38..40].copy_from_slice(&self.type_names_count.to_le_bytes());
        bytes[40..42].copy_from_slice(&self.entrypoints_count.to_le_bytes());
        bytes[42..44].copy_from_slice(&self.transitions_count.to_le_bytes());
        bytes[44..64].copy_from_slice(&self._reserved);
        bytes
    }

    pub fn validate_magic(&self) -> bool {
        self.magic == MAGIC
    }

    pub fn validate_version(&self) -> bool {
        self.version == VERSION
    }

    /// Compute section offsets from counts and blob sizes.
    ///
    /// Section order (all 64-byte aligned):
    /// Header → StringBlob → RegexBlob → StringTable → RegexTable →
    /// NodeTypes → NodeFields → Trivia → TypeDefs → TypeMembers →
    /// TypeNames → Entrypoints → Transitions
    pub fn compute_offsets(&self) -> SectionOffsets {
        let align = SECTION_ALIGN as u32;

        // Blobs first (right after header)
        let str_blob = align; // 64
        let regex_blob = align_up(str_blob + self.str_blob_size, align);

        // Tables after blobs
        let str_table = align_up(regex_blob + self.regex_blob_size, align);
        let str_table_size = (self.str_table_count as u32 + 1) * 4;

        let regex_table = align_up(str_table + str_table_size, align);
        let regex_table_size = (self.regex_table_count as u32 + 1) * 8;

        // Symbol sections
        let node_types = align_up(regex_table + regex_table_size, align);
        let node_types_size = self.node_types_count as u32 * 4;

        let node_fields = align_up(node_types + node_types_size, align);
        let node_fields_size = self.node_fields_count as u32 * 4;

        let trivia = align_up(node_fields + node_fields_size, align);
        let trivia_size = self.trivia_count as u32 * 2;

        // Type metadata
        let type_defs = align_up(trivia + trivia_size, align);
        let type_defs_size = self.type_defs_count as u32 * 4;

        let type_members = align_up(type_defs + type_defs_size, align);
        let type_members_size = self.type_members_count as u32 * 4;

        let type_names = align_up(type_members + type_members_size, align);
        let type_names_size = self.type_names_count as u32 * 4;

        // Entry points and instructions
        let entrypoints = align_up(type_names + type_names_size, align);
        let entrypoints_size = self.entrypoints_count as u32 * 8;

        let transitions = align_up(entrypoints + entrypoints_size, align);

        SectionOffsets {
            str_blob,
            regex_blob,
            str_table,
            regex_table,
            node_types,
            node_fields,
            trivia,
            type_defs,
            type_members,
            type_names,
            entrypoints,
            transitions,
        }
    }
}

/// Round up to the next multiple of `align`.
fn align_up(value: u32, align: u32) -> u32 {
    (value + align - 1) & !(align - 1)
}
