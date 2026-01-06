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
