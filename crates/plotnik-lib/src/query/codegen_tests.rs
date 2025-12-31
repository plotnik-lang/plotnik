//! Tests for bytecode emission.

use indoc::indoc;

use crate::Query;
use crate::bytecode::{Header, MAGIC, Module, QTypeId, VERSION};
use crate::query::codegen::{StringTableBuilder, TypeTableBuilder};

#[test]
fn emit_minimal_query() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);

    // Verify header
    assert!(bytes.len() >= 64);
    let header = Header::from_bytes(&bytes);
    assert_eq!(header.magic, MAGIC);
    assert_eq!(header.version, VERSION);
    assert_eq!(header.total_size as usize, bytes.len());

    // Should have 1 entrypoint
    assert_eq!(header.entrypoints_count, 1);

    // Should have at least one string (the definition name "Test")
    assert!(header.str_table_count >= 1);

    // Should have at least one node type ("identifier")
    assert!(header.node_types_count >= 1);
}

#[test]
fn emit_roundtrip_via_module() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).expect("load module");

    // Verify we can read back the strings
    assert!(module.header().str_table_count >= 1);

    // Verify we can read back entrypoints
    let entrypoints = module.entrypoints();
    assert_eq!(entrypoints.len(), 1);

    // Verify we can read the entrypoint name
    let ep = entrypoints.get(0);
    let name = module.strings().get(ep.name);
    assert_eq!(name, "Test");
}

#[test]
fn emit_multiple_definitions() {
    let input = indoc! {r#"
        Foo = (identifier) @id
        Bar = (string) @str
    "#};

    let bytes = Query::expect_valid_linked_bytes(input);
    let header = Header::from_bytes(&bytes);

    // Should have 2 entrypoints
    assert_eq!(header.entrypoints_count, 2);

    // Entrypoints preserve definition order
    let module = Module::from_bytes(bytes).expect("load module");
    let entrypoints = module.entrypoints();

    let ep0 = entrypoints.get(0);
    let ep1 = entrypoints.get(1);

    let name0 = module.strings().get(ep0.name);
    let name1 = module.strings().get(ep1.name);

    assert_eq!(name0, "Foo"); // Foo defined first
    assert_eq!(name1, "Bar");
}

#[test]
fn emit_with_field_constraint() {
    let input = "Test = (function_declaration name: (identifier) @name)";

    let bytes = Query::expect_valid_linked_bytes(input);
    let header = Header::from_bytes(&bytes);

    // Should have at least one field ("name")
    assert!(header.node_fields_count >= 1);

    let module = Module::from_bytes(bytes).expect("load module");
    let fields = module.node_fields();

    // Find the "name" field
    let has_name_field = (0..fields.len()).any(|i| {
        let f = fields.get(i);
        module.strings().get(f.name) == "name"
    });
    assert!(has_name_field, "should have 'name' field");
}

#[test]
fn emit_with_struct_type() {
    let input = indoc! {r#"
        Test = (function_declaration
            name: (identifier) @name
            body: (_) @body)
    "#};

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).expect("load module");
    let types = module.types();

    // Should have type definitions for the struct
    // The struct has 2 fields, so we expect type members
    assert!(types.defs_count() >= 1 || types.members_count() >= 2);
}

#[test]
fn emit_checksum_is_valid() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let header = Header::from_bytes(&bytes);

    // Verify checksum
    let computed = crc32fast::hash(&bytes[64..]);
    assert_eq!(header.checksum, computed, "checksum mismatch");
}

#[test]
fn emit_sections_are_aligned() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let header = Header::from_bytes(&bytes);

    // All section offsets should be 64-byte aligned
    assert_eq!(header.str_blob_offset % 64, 0, "str_blob not aligned");
    assert_eq!(header.str_table_offset % 64, 0, "str_table not aligned");
    assert_eq!(header.node_types_offset % 64, 0, "node_types not aligned");
    assert_eq!(header.node_fields_offset % 64, 0, "node_fields not aligned");
    assert_eq!(header.trivia_offset % 64, 0, "trivia not aligned");
    assert_eq!(header.type_meta_offset % 64, 0, "type_meta not aligned");
    assert_eq!(header.entrypoints_offset % 64, 0, "entrypoints not aligned");
    assert_eq!(header.transitions_offset % 64, 0, "transitions not aligned");
}

// Builder API tests - these test internal APIs directly

#[test]
fn string_table_builder_deduplicates() {
    use plotnik_core::Interner;

    let mut interner = Interner::new();
    let sym1 = interner.intern("foo");
    let sym2 = interner.intern("bar");
    let sym3 = interner.intern("foo"); // Same as sym1

    let mut builder = StringTableBuilder::new();
    let id1 = builder.get_or_intern(sym1, &interner).expect("id1");
    let id2 = builder.get_or_intern(sym2, &interner).expect("id2");
    let id3 = builder.get_or_intern(sym3, &interner).expect("id3");

    assert_eq!(id1, id3); // Same symbol -> same StringId
    assert_ne!(id1, id2); // Different symbols -> different StringIds
    // 3 strings: easter egg at index 0, plus 2 unique user strings
    assert_eq!(builder.len(), 3);
}

#[test]
fn string_table_builder_intern_str() {
    let mut builder = StringTableBuilder::new();

    let id1 = builder.intern_str("hello");
    let id2 = builder.intern_str("world");
    let id3 = builder.intern_str("hello"); // Duplicate

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    // 3 strings: easter egg at index 0, plus 2 unique user strings
    assert_eq!(builder.len(), 3);
}

#[test]
fn type_table_builder_builtins() {
    use crate::query::type_check::{TYPE_NODE, TYPE_STRING, TYPE_VOID};

    let mut builder = TypeTableBuilder::new();

    // Build with empty context
    let type_ctx = crate::query::type_check::TypeContext::new();
    let interner = plotnik_core::Interner::new();
    let mut strings = StringTableBuilder::new();

    builder
        .build(&type_ctx, &interner, &mut strings)
        .expect("build");

    // Builtins should be mapped
    assert_eq!(builder.get(TYPE_VOID), Some(QTypeId::VOID));
    assert_eq!(builder.get(TYPE_NODE), Some(QTypeId::NODE));
    assert_eq!(builder.get(TYPE_STRING), Some(QTypeId::STRING));
}

// Anchor bytecode emission tests

#[test]
fn emit_anchor_between_siblings() {
    // Anchor between two named nodes should generate NextSkip
    let input = "Test = (parent (a) . (b))";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "Test"
    S02 "parent"
    S03 "b"
    S04 "a"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 0)  ; {  }

    [types.members]

    [types.names]
    N0 = (S01, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (parent)                         03
      03  *↓   (a)                              04
      04  ~    (b)                              05
      05  *↑¹                                   ◼
    "#);
}

#[test]
fn emit_anchor_first_child() {
    // Leading anchor should generate DownSkip
    let input = "Test = (parent . (first))";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "Test"
    S02 "parent"
    S03 "first"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 0)  ; {  }

    [types.members]

    [types.names]
    N0 = (S01, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (parent)                         03
      03  ~↓   (first)                          04
      04  *↑¹                                   ◼
    "#);
}

#[test]
fn emit_anchor_last_child() {
    // Trailing anchor should generate UpSkipTrivia
    let input = "Test = (parent (last) .)";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "Test"
    S02 "parent"
    S03 "last"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 0)  ; {  }

    [types.members]

    [types.names]
    N0 = (S01, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (parent)                         03
      03  *↓   (last)                           04
      04  ~↑¹                                   ◼
    "#);
}

#[test]
fn emit_anchor_with_anonymous_node() {
    // Anchor with anonymous node should generate NextExact
    let input = r#"Test = (parent "+" . (next))"#;

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "Test"
    S02 "parent"
    S03 "next"
    S04 "+"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 0)  ; {  }

    [types.members]

    [types.names]
    N0 = (S01, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (parent)                         03
      03  *↓   (+)                              04
      04  .    (next)                           05
      05  *↑¹                                   ◼
    "#);
}

#[test]
fn emit_no_anchor_uses_next() {
    // No anchor between siblings uses Next
    let input = "Test = (parent (a) (b))";

    let res = Query::expect_valid_bytecode(input);

    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "Test"
    S02 "parent"
    S03 "b"
    S04 "a"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 0)  ; {  }

    [types.members]

    [types.names]
    N0 = (S01, T03)  ; Test

    [entry]
    Test = 01 :: T03

    [code]
      00   𝜀                                    ◼

    Test:
      01   𝜀                                    02
      02  *↓   (parent)                         03
      03  *↓   (a)                              04
      04  *    (b)                              05
      05  *↑¹                                   ◼
    "#);
}
