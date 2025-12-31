//! Bytecode file header (64 bytes).

use super::{MAGIC, VERSION};

/// Header flags (bit field).
pub mod flags {
    /// Bit 0: If set, bytecode is linked (instructions contain NodeTypeId/NodeFieldId).
    /// If clear, bytecode is unlinked (instructions contain StringId references).
    pub const LINKED: u16 = 0x0001;
}

/// File header - first 64 bytes of the bytecode file.
///
/// Note: TypeMeta sub-section counts are stored in the TypeMetaHeader,
/// not in the main header. See type_meta.rs for details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, align(64))]
pub struct Header {
    /// Magic bytes: b"PTKQ"
    pub magic: [u8; 4],
    /// Format version (currently 1)
    pub version: u32,
    /// CRC32 checksum of everything after the header
    pub checksum: u32,
    /// Total file size in bytes
    pub total_size: u32,

    // Section offsets (absolute byte offsets)
    pub str_blob_offset: u32,
    pub str_table_offset: u32,
    pub node_types_offset: u32,
    pub node_fields_offset: u32,
    pub trivia_offset: u32,
    pub type_meta_offset: u32,
    pub entrypoints_offset: u32,
    pub transitions_offset: u32,

    // Element counts (type counts are in TypeMetaHeader at type_meta_offset)
    pub str_table_count: u16,
    pub node_types_count: u16,
    pub node_fields_count: u16,
    pub trivia_count: u16,
    pub entrypoints_count: u16,
    pub transitions_count: u16,
    /// Header flags (see `flags` module for bit definitions).
    pub flags: u16,
    /// Padding to maintain 64-byte size.
    pub(crate) _pad: u16,
}

const _: () = assert!(std::mem::size_of::<Header>() == 64);

impl Default for Header {
    fn default() -> Self {
        Self {
            magic: MAGIC,
            version: VERSION,
            checksum: 0,
            total_size: 0,
            str_blob_offset: 0,
            str_table_offset: 0,
            node_types_offset: 0,
            node_fields_offset: 0,
            trivia_offset: 0,
            type_meta_offset: 0,
            entrypoints_offset: 0,
            transitions_offset: 0,
            str_table_count: 0,
            node_types_count: 0,
            node_fields_count: 0,
            trivia_count: 0,
            entrypoints_count: 0,
            transitions_count: 0,
            flags: 0,
            _pad: 0,
        }
    }
}

impl Header {
    /// Decode header from 64 bytes.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 64, "header too short");

        Self {
            magic: [bytes[0], bytes[1], bytes[2], bytes[3]],
            version: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            checksum: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            total_size: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            str_blob_offset: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            str_table_offset: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            node_types_offset: u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            node_fields_offset: u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]),
            trivia_offset: u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]),
            type_meta_offset: u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]),
            entrypoints_offset: u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            transitions_offset: u32::from_le_bytes([bytes[44], bytes[45], bytes[46], bytes[47]]),
            str_table_count: u16::from_le_bytes([bytes[48], bytes[49]]),
            node_types_count: u16::from_le_bytes([bytes[50], bytes[51]]),
            node_fields_count: u16::from_le_bytes([bytes[52], bytes[53]]),
            trivia_count: u16::from_le_bytes([bytes[54], bytes[55]]),
            entrypoints_count: u16::from_le_bytes([bytes[56], bytes[57]]),
            transitions_count: u16::from_le_bytes([bytes[58], bytes[59]]),
            flags: u16::from_le_bytes([bytes[60], bytes[61]]),
            _pad: u16::from_le_bytes([bytes[62], bytes[63]]),
        }
    }

    /// Encode header to 64 bytes.
    pub fn to_bytes(&self) -> [u8; 64] {
        let mut bytes = [0u8; 64];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.total_size.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.str_blob_offset.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.str_table_offset.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.node_types_offset.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.node_fields_offset.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.trivia_offset.to_le_bytes());
        bytes[36..40].copy_from_slice(&self.type_meta_offset.to_le_bytes());
        bytes[40..44].copy_from_slice(&self.entrypoints_offset.to_le_bytes());
        bytes[44..48].copy_from_slice(&self.transitions_offset.to_le_bytes());
        bytes[48..50].copy_from_slice(&self.str_table_count.to_le_bytes());
        bytes[50..52].copy_from_slice(&self.node_types_count.to_le_bytes());
        bytes[52..54].copy_from_slice(&self.node_fields_count.to_le_bytes());
        bytes[54..56].copy_from_slice(&self.trivia_count.to_le_bytes());
        bytes[56..58].copy_from_slice(&self.entrypoints_count.to_le_bytes());
        bytes[58..60].copy_from_slice(&self.transitions_count.to_le_bytes());
        bytes[60..62].copy_from_slice(&self.flags.to_le_bytes());
        bytes[62..64].copy_from_slice(&self._pad.to_le_bytes());
        bytes
    }

    pub fn validate_magic(&self) -> bool {
        self.magic == MAGIC
    }

    pub fn validate_version(&self) -> bool {
        self.version == VERSION
    }

    /// Returns true if the bytecode is linked (contains resolved grammar IDs).
    pub fn is_linked(&self) -> bool {
        self.flags & flags::LINKED != 0
    }

    /// Set the linked flag.
    pub fn set_linked(&mut self, linked: bool) {
        if linked {
            self.flags |= flags::LINKED;
        } else {
            self.flags &= !flags::LINKED;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_size() {
        assert_eq!(std::mem::size_of::<Header>(), 64);
    }

    #[test]
    fn header_default() {
        let h = Header::default();
        assert!(h.validate_magic());
        assert!(h.validate_version());
        assert_eq!(h.total_size, 0);
    }

    #[test]
    fn header_roundtrip() {
        let h = Header {
            magic: MAGIC,
            version: VERSION,
            checksum: 0x12345678,
            total_size: 1024,
            str_blob_offset: 64,
            str_table_offset: 128,
            node_types_offset: 192,
            node_fields_offset: 256,
            trivia_offset: 320,
            type_meta_offset: 384,
            entrypoints_offset: 448,
            transitions_offset: 512,
            str_table_count: 10,
            node_types_count: 20,
            node_fields_count: 5,
            trivia_count: 2,
            entrypoints_count: 1,
            transitions_count: 15,
            ..Default::default()
        };

        let bytes = h.to_bytes();
        assert_eq!(bytes.len(), 64);

        let decoded = Header::from_bytes(&bytes);
        assert_eq!(decoded, h);
    }

    #[test]
    fn header_linked_flag() {
        let mut h = Header::default();
        assert!(!h.is_linked());

        h.set_linked(true);
        assert!(h.is_linked());
        assert_eq!(h.flags, flags::LINKED);

        h.set_linked(false);
        assert!(!h.is_linked());
        assert_eq!(h.flags, 0);
    }

    #[test]
    fn header_flags_roundtrip() {
        let mut h = Header::default();
        h.set_linked(true);

        let bytes = h.to_bytes();
        let decoded = Header::from_bytes(&bytes);

        assert!(decoded.is_linked());
        assert_eq!(decoded.flags, flags::LINKED);
    }
}
