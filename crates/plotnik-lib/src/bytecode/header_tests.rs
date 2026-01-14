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
        str_blob_size: 100,
        regex_blob_size: 256,
        str_table_count: 10,
        regex_table_count: 3,
        node_types_count: 20,
        node_fields_count: 5,
        trivia_count: 2,
        type_defs_count: 8,
        type_members_count: 12,
        type_names_count: 4,
        entrypoints_count: 1,
        transitions_count: 15,
        flags: 0,
        _reserved: [0; 18],
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

#[test]
fn compute_offsets_empty() {
    let h = Header::default();
    let offsets = h.compute_offsets();

    // New order: blobs first, then tables
    // All sections 64-byte aligned. With 0 counts, each table still has 1 sentinel entry (4 bytes)
    assert_eq!(offsets.str_blob, 64);            // after header
    assert_eq!(offsets.regex_blob, 64);          // 64 + align(0) = 64
    assert_eq!(offsets.str_table, 64);           // 64 + align(0) = 64
    assert_eq!(offsets.regex_table, 128);        // 64 + align(4) = 128
    assert_eq!(offsets.node_types, 192);         // 128 + align(4) = 192
    assert_eq!(offsets.node_fields, 192);        // 192 + align(0) = 192
    assert_eq!(offsets.trivia, 192);
    assert_eq!(offsets.type_defs, 192);
    assert_eq!(offsets.type_members, 192);
    assert_eq!(offsets.type_names, 192);
    assert_eq!(offsets.entrypoints, 192);
    assert_eq!(offsets.transitions, 192);
}

#[test]
fn compute_offsets_with_data() {
    let h = Header {
        str_table_count: 5,     // (5+1)*4 = 24 bytes
        regex_table_count: 2,   // (2+1)*4 = 12 bytes
        node_types_count: 10,   // 10*4 = 40 bytes
        node_fields_count: 5,   // 5*4 = 20 bytes
        trivia_count: 3,        // 3*2 = 6 bytes
        type_defs_count: 8,     // 8*4 = 32 bytes
        type_members_count: 12, // 12*4 = 48 bytes
        type_names_count: 4,    // 4*4 = 16 bytes
        entrypoints_count: 2,   // 2*8 = 16 bytes
        transitions_count: 20,  // 20*8 = 160 bytes
        str_blob_size: 100,
        regex_blob_size: 128,
        ..Default::default()
    };

    let offsets = h.compute_offsets();

    // New order: blobs first, then tables. All offsets 64-byte aligned.
    assert_eq!(offsets.str_blob, 64);            // header end
    assert_eq!(offsets.regex_blob, 192);         // 64 + 100 = 164 → 192
    assert_eq!(offsets.str_table, 320);          // 192 + 128 = 320 (aligned)
    assert_eq!(offsets.regex_table, 384);        // 320 + 24 = 344 → 384
    assert_eq!(offsets.node_types, 448);         // 384 + 12 = 396 → 448
    assert_eq!(offsets.node_fields, 512);        // 448 + 40 = 488 → 512
    assert_eq!(offsets.trivia, 576);             // 512 + 20 = 532 → 576
    assert_eq!(offsets.type_defs, 640);          // 576 + 6 = 582 → 640
    assert_eq!(offsets.type_members, 704);       // 640 + 32 = 672 → 704
    assert_eq!(offsets.type_names, 768);         // 704 + 48 = 752 → 768
    assert_eq!(offsets.entrypoints, 832);        // 768 + 16 = 784 → 832
    assert_eq!(offsets.transitions, 896);        // 832 + 16 = 848 → 896
}
