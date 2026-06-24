use crate::bytecode::{StringId, deserialize_dfa};
use regex_automata::Input;
use regex_automata::dfa::Automaton;

use crate::compiler::emit::tables::RegexTableBuilder;

use super::regex_table::intern;

#[test]
fn intern_and_lookup() {
    let mut builder = RegexTableBuilder::new();

    let str1 = StringId::try_from(1).unwrap();
    let str2 = StringId::try_from(2).unwrap();

    let id1 = intern(&mut builder, "foo", str1).unwrap();
    let id2 = intern(&mut builder, "bar", str2).unwrap();
    let id3 = intern(&mut builder, "foo", str1).unwrap(); // duplicate

    assert_eq!(u16::from(id1), 1); // 0 is reserved
    assert_eq!(u16::from(id2), 2);
    assert_eq!(id3, id1); // same StringId returns same regex ID

    assert_eq!(builder.lookup(str1), Some(id1));
    assert_eq!(builder.lookup(str2), Some(id2));
    assert_eq!(builder.lookup(StringId::try_from(99).unwrap()), None);
}

#[test]
fn emit_and_deserialize() {
    let mut builder = RegexTableBuilder::new();
    intern(&mut builder, "hello", StringId::try_from(1).unwrap()).unwrap();
    intern(&mut builder, "world", StringId::try_from(2).unwrap()).unwrap();

    let (blob, table) = builder.emit();

    // Table should have 3 entries + sentinel, 8 bytes each: (string_id u16 | reserved u16 | offset u32)
    assert_eq!(table.len(), 4 * 8);

    // Read first regex entry (index 1): string_id at bytes 8-9, offset at bytes 12-15
    let string_id1 = u16::from_le_bytes([table[8], table[9]]);
    let offset1 = u32::from_le_bytes([table[12], table[13], table[14], table[15]]) as usize;

    // Read second regex entry (index 2)
    let offset2 = u32::from_le_bytes([table[20], table[21], table[22], table[23]]) as usize;

    // Verify string_id stored correctly
    assert_eq!(string_id1, 1);

    // Deserialize and test first regex
    let dfa1 = deserialize_dfa(&blob[offset1..offset2]).unwrap();
    assert!(
        dfa1.try_search_fwd(&Input::new("hello"))
            .ok()
            .flatten()
            .is_some()
    );
    assert!(
        dfa1.try_search_fwd(&Input::new("world"))
            .ok()
            .flatten()
            .is_none()
    );
}

#[test]
fn escaped_slash_pattern() {
    let mut builder = RegexTableBuilder::new();
    // Pattern "a\/b" should match literal "a/b"
    let id = intern(&mut builder, r"a\/b", StringId::try_from(1).unwrap()).unwrap();
    assert_eq!(u16::from(id), 1);

    let (blob, table) = builder.emit();

    // Read offsets from table (8 bytes per entry)
    let offset1 = u32::from_le_bytes([table[12], table[13], table[14], table[15]]) as usize;
    let offset2 = u32::from_le_bytes([table[20], table[21], table[22], table[23]]) as usize;

    let dfa = deserialize_dfa(&blob[offset1..offset2]).unwrap();
    assert!(
        dfa.try_search_fwd(&Input::new("a/b"))
            .ok()
            .flatten()
            .is_some()
    );
    assert!(
        dfa.try_search_fwd(&Input::new("ab"))
            .ok()
            .flatten()
            .is_none()
    );
}

#[test]
fn empty_builder() {
    let builder = RegexTableBuilder::new();
    assert_eq!(builder.len(), 1); // just reserved slot
}

#[test]
fn default_builder_preserves_reserved_slot() {
    let builder = RegexTableBuilder::default();
    assert_eq!(builder.len(), 1);
}
