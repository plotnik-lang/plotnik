//! Tests for bytecode emission.

use plotnik_langs::{Lang, from_name};

use crate::bytecode::{Header, MAGIC, Module, QTypeId, VERSION};
use crate::query::QueryBuilder;
use crate::query::emit::{StringTableBuilder, TypeTableBuilder};

fn javascript() -> Lang {
    from_name("javascript").expect("javascript lang")
}

fn emit_query(src: &str) -> Vec<u8> {
    QueryBuilder::one_liner(src)
        .parse()
        .expect("parse")
        .analyze()
        .link(&javascript())
        .emit()
        .expect("emit")
}

#[test]
fn emit_minimal_query() {
    let bytes = emit_query("Test = (identifier) @id");

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
    let bytes = emit_query("Test = (identifier) @id");

    // Load the bytes as a Module
    let module = Module::from_bytes(bytes).expect("load module");

    // Verify we can read back the strings
    let strings = module.strings();
    assert!(module.header().str_table_count >= 1);

    // Verify we can read back entrypoints
    let entrypoints = module.entrypoints();
    assert_eq!(entrypoints.len(), 1);

    // Verify we can read the entrypoint name
    let ep = entrypoints.get(0);
    let name = strings.get(ep.name);
    assert_eq!(name, "Test");
}

#[test]
fn emit_multiple_definitions() {
    let bytes = emit_query(
        r#"
        Foo = (identifier) @id
        Bar = (string) @str
        "#,
    );

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
    let bytes = emit_query("Test = (function_declaration name: (identifier) @name)");

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
    // This should produce a struct type with two fields
    let bytes =
        emit_query("Test = (function_declaration name: (identifier) @name body: (_) @body)");

    // Load the module to check type metadata
    let module = Module::from_bytes(bytes).expect("load module");
    let types = module.types();

    // Should have type definitions for the struct
    // The struct has 2 fields, so we expect type members
    assert!(types.defs_count() >= 1 || types.members_count() >= 2);
}

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
    assert_eq!(builder.len(), 2); // Only 2 unique strings
}

#[test]
fn string_table_builder_intern_str() {
    let mut builder = StringTableBuilder::new();

    let id1 = builder.intern_str("hello");
    let id2 = builder.intern_str("world");
    let id3 = builder.intern_str("hello"); // Duplicate

    assert_eq!(id1, id3);
    assert_ne!(id1, id2);
    assert_eq!(builder.len(), 2);
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

#[test]
fn emit_checksum_is_valid() {
    let bytes = emit_query("Test = (identifier) @id");

    let header = Header::from_bytes(&bytes);

    // Verify checksum
    let computed = crc32fast::hash(&bytes[64..]);
    assert_eq!(header.checksum, computed, "checksum mismatch");
}

#[test]
fn emit_sections_are_aligned() {
    let bytes = emit_query("Test = (identifier) @id");

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

#[test]
fn debug_recursive_quantified() {
    use crate::SourceMap;
    use crate::bytecode::QTypeId;

    let src = "Item = (item (Item)* @children)";
    let source_map = SourceMap::one_liner(src);
    let query = crate::query::QueryBuilder::new(source_map)
        .parse()
        .unwrap()
        .analyze();

    eprintln!("=== TypeContext ===");
    for (id, kind) in query.type_context().iter_types() {
        eprintln!("TypeId {:?}: {:?}", id, kind);
    }

    for (def_id, type_id) in query.type_context().iter_def_types() {
        let name_sym = query.type_context().def_name_sym(def_id);
        let name = query.interner().resolve(name_sym);
        eprintln!("DefId {:?}: {} -> TypeId {:?}", def_id, name, type_id);
    }

    let bytecode = query.emit().expect("emit");
    let module = Module::from_bytes(bytecode).expect("load");

    eprintln!("\n=== Bytecode ===");
    eprintln!("TypeDefs count: {}", module.types().defs_count());
    for i in 0..module.types().defs_count() {
        let def = module.types().get_def(i);
        let type_id = QTypeId::from_custom_index(i);
        eprintln!(
            "  TypeDef[{}] (id={:?}): kind={}, data={}, count={}",
            i, type_id, def.kind, def.data, def.count
        );
    }

    eprintln!("\nEntrypoints: {}", module.entrypoints().len());
    for i in 0..module.entrypoints().len() {
        let ep = module.entrypoints().get(i);
        let name = module.strings().get(ep.name);
        eprintln!("  {}: result_type = {:?}", name, ep.result_type);
    }

    eprintln!("\n=== TypeScript Output ===");
    let ts = crate::bytecode::emit::emit_typescript(&module);
    eprintln!("{}", ts);
}

#[test]
fn debug_untagged_alt() {
    use crate::SourceMap;
    use crate::bytecode::QTypeId;

    let src = "Q = [(a) @a (b) @b]";
    let source_map = SourceMap::one_liner(src);
    let query = crate::query::QueryBuilder::new(source_map)
        .parse()
        .unwrap()
        .analyze();

    // Check type context
    eprintln!("=== TypeContext ===");
    for (def_id, type_id) in query.type_context().iter_def_types() {
        let name_sym = query.type_context().def_name_sym(def_id);
        let name = query.interner().resolve(name_sym);
        eprintln!("DefId {:?}: {} -> TypeId {:?}", def_id, name, type_id);

        if let Some(tk) = query.type_context().get_type(type_id) {
            eprintln!("  TypeKind: {:?}", tk);
        }
    }

    // Emit bytecode
    let bytecode = query.emit().expect("emit");
    let module = Module::from_bytes(bytecode).expect("load");

    eprintln!("\n=== Bytecode ===");
    eprintln!("Entrypoints: {}", module.entrypoints().len());
    for i in 0..module.entrypoints().len() {
        let ep = module.entrypoints().get(i);
        let name = module.strings().get(ep.name);
        eprintln!("  {}: result_type = {:?}", name, ep.result_type);

        if let Some(def) = module.types().get(ep.result_type) {
            eprintln!(
                "    TypeDef: kind={}, data={}, count={}",
                def.kind, def.data, def.count
            );
        } else if ep.result_type.is_builtin() {
            eprintln!("    Builtin type: {:?}", ep.result_type);
        }
    }

    eprintln!("\nTypeDefs count: {}", module.types().defs_count());
    for i in 0..module.types().defs_count() {
        let def = module.types().get_def(i);
        let type_id = QTypeId::from_custom_index(i);
        eprintln!(
            "  TypeDef[{}] (id={:?}): kind={}, data={}, count={}",
            i, type_id, def.kind, def.data, def.count
        );
    }

    eprintln!("\nTypeMembers count: {}", module.types().members_count());
    for i in 0..module.types().members_count() {
        let m = module.types().get_member(i);
        let name = module.strings().get(m.name);
        eprintln!(
            "  TypeMember[{}]: name={}, type_id={:?}",
            i, name, m.type_id
        );
    }

    eprintln!("\nTypeNames count: {}", module.types().names_count());
    for i in 0..module.types().names_count() {
        let tn = module.types().get_name(i);
        let name = module.strings().get(tn.name);
        eprintln!("  TypeName[{}]: name={}, type_id={:?}", i, name, tn.type_id);
    }

    eprintln!("\n=== TypeScript Output ===");
    let ts = crate::bytecode::emit::emit_typescript(&module);
    eprintln!("{}", ts);
}
