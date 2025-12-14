//! Serialization and deserialization for compiled queries.
//!
//! Binary format (see ADR-0004):
//! ```text
//! Header (64 bytes):
//!   magic: [u8; 4]           b"PLNK"
//!   version: u32             format version
//!   checksum: u32            CRC32(header[12..64] || buffer_data)
//!   buffer_len: u32
//!   successors_offset: u32
//!   effects_offset: u32
//!   negated_fields_offset: u32
//!   string_refs_offset: u32
//!   string_bytes_offset: u32
//!   type_defs_offset: u32
//!   type_members_offset: u32
//!   entrypoints_offset: u32
//!   trivia_kinds_offset: u32
//!   _reserved: [u8; 12]
//! ```

use std::io::{Read, Write};

use super::compiled::{CompiledQuery, CompiledQueryBuffer, FORMAT_VERSION, MAGIC};

/// Header size in bytes (64 bytes for cache-line alignment).
pub const HEADER_SIZE: usize = 64;

/// Serialization error.
#[derive(Debug, Clone)]
pub enum SerializeError {
    /// Invalid magic bytes.
    InvalidMagic([u8; 4]),
    /// Version mismatch (expected, found).
    VersionMismatch { expected: u32, found: u32 },
    /// Checksum mismatch (expected, found).
    ChecksumMismatch { expected: u32, found: u32 },
    /// IO error message.
    Io(String),
    /// Header too short.
    HeaderTooShort,
    /// Buffer alignment error.
    AlignmentError,
}

impl std::fmt::Display for SerializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SerializeError::InvalidMagic(m) => {
                write!(f, "invalid magic: {:?}", m)
            }
            SerializeError::VersionMismatch { expected, found } => {
                write!(
                    f,
                    "version mismatch: expected {}, found {}",
                    expected, found
                )
            }
            SerializeError::ChecksumMismatch { expected, found } => {
                write!(
                    f,
                    "checksum mismatch: expected {:08x}, found {:08x}",
                    expected, found
                )
            }
            SerializeError::Io(msg) => write!(f, "io error: {}", msg),
            SerializeError::HeaderTooShort => write!(f, "header too short"),
            SerializeError::AlignmentError => write!(f, "buffer alignment error"),
        }
    }
}

impl std::error::Error for SerializeError {}

impl From<std::io::Error> for SerializeError {
    fn from(e: std::io::Error) -> Self {
        SerializeError::Io(e.to_string())
    }
}

/// Result type for serialization operations.
pub type SerializeResult<T> = Result<T, SerializeError>;

/// Computes CRC32 checksum.
fn crc32(data: &[u8]) -> u32 {
    // Simple CRC32 implementation (IEEE polynomial)
    const CRC32_TABLE: [u32; 256] = generate_crc32_table();

    let mut crc: u32 = 0xFFFFFFFF;
    for &byte in data {
        let index = ((crc ^ byte as u32) & 0xFF) as usize;
        crc = CRC32_TABLE[index] ^ (crc >> 8);
    }
    !crc
}

const fn generate_crc32_table() -> [u32; 256] {
    const POLYNOMIAL: u32 = 0xEDB88320;
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut j = 0;
        while j < 8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ POLYNOMIAL;
            } else {
                crc >>= 1;
            }
            j += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Serialized header structure (64 bytes, matching ADR-0004).
///
/// Large counts (transition, successor, effect) are computed from offsets.
/// Small counts are stored in the reserved area since they can't be reliably
/// computed due to alignment padding.
#[repr(C)]
struct Header {
    magic: [u8; 4],
    version: u32,
    checksum: u32,
    buffer_len: u32,
    successors_offset: u32,
    effects_offset: u32,
    negated_fields_offset: u32,
    string_refs_offset: u32,
    string_bytes_offset: u32,
    type_defs_offset: u32,
    type_members_offset: u32,
    entrypoints_offset: u32,
    trivia_kinds_offset: u32,
    // Counts stored in reserved area (12 bytes = 6 x u16)
    negated_field_count: u16,
    string_ref_count: u16,
    type_def_count: u16,
    type_member_count: u16,
    entrypoint_count: u16,
    trivia_kind_count: u16,
}

const _: () = assert!(std::mem::size_of::<Header>() == HEADER_SIZE);

impl Header {
    fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut bytes = [0u8; HEADER_SIZE];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..8].copy_from_slice(&self.version.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.checksum.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.buffer_len.to_le_bytes());
        bytes[16..20].copy_from_slice(&self.successors_offset.to_le_bytes());
        bytes[20..24].copy_from_slice(&self.effects_offset.to_le_bytes());
        bytes[24..28].copy_from_slice(&self.negated_fields_offset.to_le_bytes());
        bytes[28..32].copy_from_slice(&self.string_refs_offset.to_le_bytes());
        bytes[32..36].copy_from_slice(&self.string_bytes_offset.to_le_bytes());
        bytes[36..40].copy_from_slice(&self.type_defs_offset.to_le_bytes());
        bytes[40..44].copy_from_slice(&self.type_members_offset.to_le_bytes());
        bytes[44..48].copy_from_slice(&self.entrypoints_offset.to_le_bytes());
        bytes[48..52].copy_from_slice(&self.trivia_kinds_offset.to_le_bytes());
        // Counts in reserved area
        bytes[52..54].copy_from_slice(&self.negated_field_count.to_le_bytes());
        bytes[54..56].copy_from_slice(&self.string_ref_count.to_le_bytes());
        bytes[56..58].copy_from_slice(&self.type_def_count.to_le_bytes());
        bytes[58..60].copy_from_slice(&self.type_member_count.to_le_bytes());
        bytes[60..62].copy_from_slice(&self.entrypoint_count.to_le_bytes());
        bytes[62..64].copy_from_slice(&self.trivia_kind_count.to_le_bytes());
        bytes
    }

    fn from_bytes(bytes: &[u8; HEADER_SIZE]) -> Self {
        Self {
            magic: bytes[0..4].try_into().unwrap(),
            version: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            checksum: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            buffer_len: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            successors_offset: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            effects_offset: u32::from_le_bytes(bytes[20..24].try_into().unwrap()),
            negated_fields_offset: u32::from_le_bytes(bytes[24..28].try_into().unwrap()),
            string_refs_offset: u32::from_le_bytes(bytes[28..32].try_into().unwrap()),
            string_bytes_offset: u32::from_le_bytes(bytes[32..36].try_into().unwrap()),
            type_defs_offset: u32::from_le_bytes(bytes[36..40].try_into().unwrap()),
            type_members_offset: u32::from_le_bytes(bytes[40..44].try_into().unwrap()),
            entrypoints_offset: u32::from_le_bytes(bytes[44..48].try_into().unwrap()),
            trivia_kinds_offset: u32::from_le_bytes(bytes[48..52].try_into().unwrap()),
            negated_field_count: u16::from_le_bytes(bytes[52..54].try_into().unwrap()),
            string_ref_count: u16::from_le_bytes(bytes[54..56].try_into().unwrap()),
            type_def_count: u16::from_le_bytes(bytes[56..58].try_into().unwrap()),
            type_member_count: u16::from_le_bytes(bytes[58..60].try_into().unwrap()),
            entrypoint_count: u16::from_le_bytes(bytes[60..62].try_into().unwrap()),
            trivia_kind_count: u16::from_le_bytes(bytes[62..64].try_into().unwrap()),
        }
    }
}

/// Serializes a compiled query to a writer.
pub fn serialize<W: Write>(query: &CompiledQuery, mut writer: W) -> SerializeResult<()> {
    let offsets = query.offsets();
    let buffer = query.buffer();

    // Build header (without checksum first)
    let mut header = Header {
        magic: MAGIC,
        version: FORMAT_VERSION,
        checksum: 0, // Computed below
        buffer_len: buffer.len() as u32,
        successors_offset: offsets.successors_offset,
        effects_offset: offsets.effects_offset,
        negated_fields_offset: offsets.negated_fields_offset,
        string_refs_offset: offsets.string_refs_offset,
        string_bytes_offset: offsets.string_bytes_offset,
        type_defs_offset: offsets.type_defs_offset,
        type_members_offset: offsets.type_members_offset,
        entrypoints_offset: offsets.entrypoints_offset,
        trivia_kinds_offset: offsets.trivia_kinds_offset,
        negated_field_count: query.negated_fields().len() as u16,
        string_ref_count: query.string_refs().len() as u16,
        type_def_count: query.type_defs().len() as u16,
        type_member_count: query.type_members().len() as u16,
        entrypoint_count: query.entrypoint_count(),
        trivia_kind_count: query.trivia_kinds().len() as u16,
    };

    // Compute checksum over header[12..64] + buffer
    let header_bytes = header.to_bytes();
    let mut checksum_data = Vec::with_capacity(52 + buffer.len());
    checksum_data.extend_from_slice(&header_bytes[12..]);
    checksum_data.extend_from_slice(buffer.as_slice());
    header.checksum = crc32(&checksum_data);

    // Write header and buffer
    writer.write_all(&header.to_bytes())?;
    writer.write_all(buffer.as_slice())?;

    Ok(())
}

/// Serializes a compiled query to a byte vector.
pub fn to_bytes(query: &CompiledQuery) -> SerializeResult<Vec<u8>> {
    let mut bytes = Vec::with_capacity(HEADER_SIZE + query.buffer().len());
    serialize(query, &mut bytes)?;
    Ok(bytes)
}

/// Deserializes a compiled query from a reader.
pub fn deserialize<R: Read>(mut reader: R) -> SerializeResult<CompiledQuery> {
    // Read header
    let mut header_bytes = [0u8; HEADER_SIZE];
    reader.read_exact(&mut header_bytes)?;

    let header = Header::from_bytes(&header_bytes);

    // Verify magic
    if header.magic != MAGIC {
        return Err(SerializeError::InvalidMagic(header.magic));
    }

    // Verify version
    if header.version != FORMAT_VERSION {
        return Err(SerializeError::VersionMismatch {
            expected: FORMAT_VERSION,
            found: header.version,
        });
    }

    // Read buffer
    let buffer_len = header.buffer_len as usize;
    let mut buffer = CompiledQueryBuffer::allocate(buffer_len);
    reader.read_exact(buffer.as_mut_slice())?;

    // Verify checksum
    let mut checksum_data = Vec::with_capacity(52 + buffer_len);
    checksum_data.extend_from_slice(&header_bytes[12..]);
    checksum_data.extend_from_slice(buffer.as_slice());
    let computed_checksum = crc32(&checksum_data);

    if header.checksum != computed_checksum {
        return Err(SerializeError::ChecksumMismatch {
            expected: header.checksum,
            found: computed_checksum,
        });
    }

    // Reconstruct all counts from offsets (transitions are 64 bytes each)
    let transition_count = header.successors_offset / 64;
    let successor_count = compute_count_from_offsets(
        header.successors_offset,
        header.effects_offset,
        4, // size of TransitionId
    );
    let effect_count = compute_count_from_offsets(
        header.effects_offset,
        header.negated_fields_offset,
        4, // size of EffectOp
    );

    // Counts are read directly from header
    let negated_field_count = header.negated_field_count;
    let string_ref_count = header.string_ref_count;
    let type_def_count = header.type_def_count;
    let type_member_count = header.type_member_count;
    let entrypoint_count = header.entrypoint_count;
    let trivia_kind_count = header.trivia_kind_count;

    Ok(CompiledQuery::new(
        buffer,
        header.successors_offset,
        header.effects_offset,
        header.negated_fields_offset,
        header.string_refs_offset,
        header.string_bytes_offset,
        header.type_defs_offset,
        header.type_members_offset,
        header.entrypoints_offset,
        header.trivia_kinds_offset,
        transition_count,
        successor_count,
        effect_count,
        negated_field_count,
        string_ref_count,
        type_def_count,
        type_member_count,
        entrypoint_count,
        trivia_kind_count,
    ))
}

/// Deserializes a compiled query from a byte slice.
pub fn from_bytes(bytes: &[u8]) -> SerializeResult<CompiledQuery> {
    deserialize(std::io::Cursor::new(bytes))
}

fn compute_count_from_offsets(start: u32, end: u32, element_size: u32) -> u32 {
    if end <= start {
        return 0;
    }
    (end - start) / element_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_value() {
        // Test against known CRC32 value
        let data = b"123456789";
        let crc = crc32(data);
        assert_eq!(crc, 0xCBF43926);
    }

    #[test]
    fn header_roundtrip() {
        let header = Header {
            magic: MAGIC,
            version: FORMAT_VERSION,
            checksum: 0x12345678,
            buffer_len: 1024,
            successors_offset: 64,
            effects_offset: 128,
            negated_fields_offset: 256,
            string_refs_offset: 300,
            string_bytes_offset: 400,
            type_defs_offset: 500,
            type_members_offset: 600,
            entrypoints_offset: 700,
            trivia_kinds_offset: 800,
            negated_field_count: 5,
            string_ref_count: 8,
            type_def_count: 3,
            type_member_count: 12,
            entrypoint_count: 2,
            trivia_kind_count: 1,
        };

        let bytes = header.to_bytes();
        let parsed = Header::from_bytes(&bytes);

        assert_eq!(parsed.magic, header.magic);
        assert_eq!(parsed.version, header.version);
        assert_eq!(parsed.checksum, header.checksum);
        assert_eq!(parsed.buffer_len, header.buffer_len);
        assert_eq!(parsed.successors_offset, header.successors_offset);
        assert_eq!(parsed.trivia_kinds_offset, header.trivia_kinds_offset);
        assert_eq!(parsed.entrypoint_count, header.entrypoint_count);
        assert_eq!(parsed.type_def_count, header.type_def_count);
    }

    #[test]
    fn invalid_magic_rejected() {
        let mut data = vec![0u8; HEADER_SIZE + 64];
        data[0..4].copy_from_slice(b"NOTM");

        let result = from_bytes(&data);
        assert!(matches!(result, Err(SerializeError::InvalidMagic(_))));
    }

    #[test]
    fn version_mismatch_rejected() {
        let mut data = vec![0u8; HEADER_SIZE + 64];
        data[0..4].copy_from_slice(&MAGIC);
        data[4..8].copy_from_slice(&999u32.to_le_bytes());

        let result = from_bytes(&data);
        assert!(matches!(
            result,
            Err(SerializeError::VersionMismatch { .. })
        ));
    }
}
