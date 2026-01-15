//! Unit tests for StringTableBuilder.

use plotnik_core::Interner;

use super::string_table::{EASTER_EGG, StringTableBuilder};

#[test]
fn new_builder_has_easter_egg_at_index_zero() {
    let builder = StringTableBuilder::new();

    assert_eq!(builder.len(), 1);
    assert!(!builder.is_empty());

    let (blob, _) = builder.emit();
    assert_eq!(std::str::from_utf8(&blob).unwrap(), EASTER_EGG);
}

#[test]
fn intern_symbol_twice_returns_same_id() {
    let mut interner = Interner::new();
    let sym = interner.intern("hello");

    let mut builder = StringTableBuilder::new();

    let id1 = builder.get_or_intern(sym, &interner).unwrap();
    let id2 = builder.get_or_intern(sym, &interner).unwrap();

    assert_eq!(id1, id2);
    assert_eq!(builder.len(), 2); // easter egg + "hello"
}

#[test]
fn intern_different_symbols_returns_different_ids() {
    let mut interner = Interner::new();
    let sym1 = interner.intern("hello");
    let sym2 = interner.intern("world");

    let mut builder = StringTableBuilder::new();

    let id1 = builder.get_or_intern(sym1, &interner).unwrap();
    let id2 = builder.get_or_intern(sym2, &interner).unwrap();

    assert_ne!(id1, id2);
    assert_eq!(builder.len(), 3); // easter egg + "hello" + "world"
}

#[test]
fn intern_str_deduplicates() {
    let mut builder = StringTableBuilder::new();

    let id1 = builder.intern_str("test");
    let id2 = builder.intern_str("test");
    let id3 = builder.intern_str("other");

    assert_eq!(id1, id2);
    assert_ne!(id1, id3);
    assert_eq!(builder.len(), 3); // easter egg + "test" + "other"
}

#[test]
fn get_returns_none_for_unknown_symbol() {
    let mut interner = Interner::new();
    let known = interner.intern("known");
    let unknown = interner.intern("unknown");

    let mut builder = StringTableBuilder::new();
    builder.get_or_intern(known, &interner).unwrap();

    assert!(builder.get(known).is_some());
    assert!(builder.get(unknown).is_none());
}

#[test]
fn validate_passes_for_normal_counts() {
    let builder = StringTableBuilder::new();

    assert!(builder.validate().is_ok());
}

#[test]
fn emit_produces_correct_format() {
    let mut builder = StringTableBuilder::new();
    builder.intern_str("abc");
    builder.intern_str("defgh");

    let (blob, table) = builder.emit();

    // Blob should contain: EASTER_EGG + "abc" + "defgh"
    let expected_blob = format!("{EASTER_EGG}abcdefgh");
    assert_eq!(blob, expected_blob.as_bytes());

    // Table should have 4 entries (3 strings + sentinel)
    // Each entry is 4 bytes (u32 le)
    assert_eq!(table.len(), 4 * 4);

    // Verify offsets
    let offsets: Vec<u32> = table
        .chunks(4)
        .map(|chunk| u32::from_le_bytes(chunk.try_into().unwrap()))
        .collect();

    assert_eq!(offsets[0], 0); // EASTER_EGG starts at 0
    assert_eq!(offsets[1], EASTER_EGG.len() as u32); // "abc" starts after easter egg
    assert_eq!(offsets[2], (EASTER_EGG.len() + 3) as u32); // "defgh" starts after "abc"
    assert_eq!(offsets[3], (EASTER_EGG.len() + 8) as u32); // sentinel
}

#[test]
fn string_not_found_error_for_unknown_symbol() {
    let mut interner = Interner::new();
    let sym = interner.intern("exists");

    // Create a different interner without the symbol
    let other_interner = Interner::new();

    let mut builder = StringTableBuilder::new();
    let result = builder.get_or_intern(sym, &other_interner);

    assert!(result.is_err());
}
