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
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (parent)                         03
      03  ↓*   (a)                              04
      04  ~    (b)                              05
      05 *↑¹                                    06
      06                                        ▶
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
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (parent)                         03
      03  ↓~   (first)                          04
      04 *↑¹                                    05
      05                                        ▶
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
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (parent)                         03
      03  ↓*   (last)                           04
      04 ~↑¹                                    05
      05                                        ▶
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
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (parent)                         03
      03  ↓*   (+)                              04
      04  .    (next)                           05
      05 *↑¹                                    06
      06                                        ▶
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
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02       (parent)                         03
      03  ↓*   (a)                              04
      04  *    (b)                              05
      05 *↑¹                                    06
      06                                        ▶
    "#);
}

// Tests for wrapper struct Set() emission (Issue 1 fix)

#[test]
fn emit_wrapper_struct_set() {
    // Wrapper struct captures should emit Set() for the wrapper field.
    // Pattern: {{inner captures} @wrapper}* @outer
    // The @wrapper capture should Set into the array element type.
    let input = "Test = {{(identifier) @id (number) @num} @row}* @rows";

    let res = Query::expect_valid_bytecode(input);

    // Verify Set(M2) is emitted for the @row wrapper field
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "num"
    S03 "row"
    S04 "rows"
    S05 "Test"
    S06 "number"
    S07 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 2)  ; { id, num }
    T04 = Struct(M2, 1)  ; { row }
    T05 = ArrayStar(T04)  ; T04*
    T06 = Struct(M3, 1)  ; { rows }

    [types.members]
    M0 = (S01, T01)  ; id: Node
    M1 = (S02, T01)  ; num: Node
    M2 = (S03, T03)  ; row: T03
    M3 = (S04, T05)  ; rows: T05

    [types.names]
    N0 = (S05, T06)  ; Test

    [entry]
    Test = 01 :: T06

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02  𝜀    [Arr]                            04
      04  𝜀                                     19, 07
      06                                        ▶
      07  𝜀    [EndArr Set(M3)]                 06
      09  𝜀    [EndObj Push]                    33
      11  𝜀    [EndObj Set(M2)]                 09
      13  *    (number) [Node Set(M1)]          11
      15       (identifier) [Node Set(M0)]      13
      17  𝜀    [Obj]                            15
      19  𝜀    [Obj]                            17
      21  𝜀    [EndObj Push]                    33
      23  𝜀    [EndObj Set(M2)]                 21
      25  *    (number) [Node Set(M1)]          23
      27       (identifier) [Node Set(M0)]      25
      29  𝜀    [Obj]                            27
      31  𝜀    [Obj]                            29
      33  𝜀                                     31, 07
    "#);
}

#[test]
fn emit_optional_wrapper_struct_set() {
    // Optional wrapper struct captures should also emit Set() for the wrapper field.
    let input = "Test = {{(identifier) @id} @inner}? @outer";

    let res = Query::expect_valid_bytecode(input);

    // Verify Set(M1) is emitted for the @inner wrapper field
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "id"
    S02 "inner"
    S03 "outer"
    S04 "Test"
    S05 "identifier"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { id }
    T04 = Struct(M1, 1)  ; { inner }
    T05 = Struct(M2, 1)  ; { outer }
    T06 = Optional(T03)  ; T03?
    T07 = Optional(T04)  ; T04?

    [types.members]
    M0 = (S01, T01)  ; id: Node
    M1 = (S02, T06)  ; inner: T06
    M2 = (S03, T07)  ; outer: T07

    [types.names]
    N0 = (S04, T05)  ; Test

    [entry]
    Test = 01 :: T05

    [code]
      00  𝜀                                     ◼

    Test:
      01  𝜀                                     02
      02  𝜀    [Obj]                            04
      04  𝜀                                     13, 15
      06                                        ▶
      07  𝜀    [EndObj Set(M2)]                 06
      09  𝜀    [EndObj Set(M1)]                 07
      11       (identifier) [Node Set(M0)]      09
      13  𝜀    [Obj]                            11
      15  𝜀    [Null Set(M1)]                   07
    "#);
}

// Tests for recursive ref structured result (Issue 2 fix)

#[test]
fn emit_recursive_ref_structured_result() {
    // Recursive refs returning structured types should not emit Node before Set.
    // The Call leaves the structured result pending, which Set directly consumes.
    let input = indoc! {r#"
        Expr = [
          Lit: (number) @value :: string
          Nested: (call_expression function: (identifier) @fn arguments: (Expr) @inner)
        ]

        Test = (program (Expr) @expr)
    "#};

    let res = Query::expect_valid_bytecode(input);

    // Verify [Set(M5)] without Node for @expr capture of recursive ref
    // Verify [Set(M2)] without Node for @inner capture of recursive ref
    insta::assert_snapshot!(res, @r#"
    [header]
    linked = false

    [strings]
    S00 "Beauty will save the world"
    S01 "value"
    S02 "fn"
    S03 "inner"
    S04 "Lit"
    S05 "Nested"
    S06 "expr"
    S07 "Expr"
    S08 "Test"
    S09 "number"
    S10 "call_expression"
    S11 "arguments"
    S12 "function"
    S13 "identifier"
    S14 "program"

    [types.defs]
    T00 = void
    T01 = Node
    T02 = str
    T03 = Struct(M0, 1)  ; { value }
    T04 = Struct(M1, 2)  ; { fn, inner }
    T05 = Enum(M3, 2)  ; Lit | Nested
    T06 = Struct(M5, 1)  ; { expr }

    [types.members]
    M0 = (S01, T02)  ; value: str
    M1 = (S02, T01)  ; fn: Node
    M2 = (S03, T05)  ; inner: Expr
    M3 = (S04, T03)  ; Lit: T03
    M4 = (S05, T04)  ; Nested: T04
    M5 = (S06, T05)  ; expr: Expr

    [types.names]
    N0 = (S07, T05)  ; Expr
    N1 = (S08, T06)  ; Test

    [entry]
    Expr = 01 :: T05
    Test = 04 :: T06

    [code]
      00  𝜀                                     ◼

    Expr:
      01  𝜀                                     02
      02  𝜀                                     19, 27

    Test:
      04  𝜀                                     05
      05       (program)                        06
      06  ↓* ▶ (Expr)                           07
      07  𝜀    [Set(M5)]                        09
      09 *↑¹                                    10
      10                                        ▶
      11  𝜀    [Set(M2)]                        13
      13 *↑¹                                    21
      14                                        ▶
      15  𝜀    [EndEnum]                        14
      17       (number) [Text Set(M0)]          15
      19  𝜀    [Enum(M3)]                       17
      21  𝜀    [EndEnum]                        14
      23  *  ▶ arguments: (Expr)                11
      24  ↓*   function: (identifier) [Node Set(M1)]23
      26       (call_expression)                24
      27  𝜀    [Enum(M4)]                       26
    "#);
}
