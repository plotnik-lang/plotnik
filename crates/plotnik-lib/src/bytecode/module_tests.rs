//! Tests for the bytecode module.

use super::*;
use crate::bytecode::nav::Nav;
use crate::bytecode::{Header, MAGIC, Match, TypeMetaHeader, VERSION};

/// Build a minimal valid bytecode for testing.
fn build_test_bytecode() -> Vec<u8> {
    // Layout (all sections 64-byte aligned):
    // [0..64)     Header
    // [64..128)   StringBlob + padding
    // [128..192)  StringTable + padding (needs 2 u32 entries: offset + sentinel)
    // [192..256)  NodeTypes + padding
    // [256..320)  NodeFields + padding
    // [320..384)  Trivia + padding
    // [384..448)  TypeMeta: TypeMetaHeader (8 bytes) + padding
    // [448..512)  TypeDefs sub-section (aligned)
    // [512..576)  TypeMembers sub-section (aligned, empty)
    // [576..640)  TypeNames sub-section (aligned, empty)
    // [640..704)  Entrypoints + padding
    // [704..768)  Transitions + padding

    let mut bytes = vec![0u8; 768];

    // String blob: "Test" at offset 0
    let str_blob_offset = 64;
    bytes[64] = b'T';
    bytes[65] = b'e';
    bytes[66] = b's';
    bytes[67] = b't';

    // String table: sequential u32 offsets with sentinel
    // Entry 0: offset 0 (start of "Test")
    // Entry 1: offset 4 (sentinel = end of blob)
    let str_table_offset = 128;
    bytes[128..132].copy_from_slice(&0u32.to_le_bytes()); // offset of string 0
    bytes[132..136].copy_from_slice(&4u32.to_le_bytes()); // sentinel (end of blob)

    // Node types: one entry (id=42, name=StringId(0))
    let node_types_offset = 192;
    bytes[192..194].copy_from_slice(&42u16.to_le_bytes());
    bytes[194..196].copy_from_slice(&0u16.to_le_bytes());

    // Node fields: one entry (id=7, name=StringId(0))
    let node_fields_offset = 256;
    bytes[256..258].copy_from_slice(&7u16.to_le_bytes());
    bytes[258..260].copy_from_slice(&0u16.to_le_bytes());

    // Trivia: one entry (node_type=100)
    let trivia_offset = 320;
    bytes[320..322].copy_from_slice(&100u16.to_le_bytes());

    // TypeMeta section
    let type_meta_offset = 384;

    // TypeMetaHeader (8 bytes): type_defs_count=1, type_members_count=0, type_names_count=0
    let type_meta_header = TypeMetaHeader {
        type_defs_count: 1,
        type_members_count: 0,
        type_names_count: 0,
        _pad: 0,
    };
    bytes[384..392].copy_from_slice(&type_meta_header.to_bytes());

    // TypeDefs sub-section at aligned offset (448)
    // One TypeDef (4 bytes): data=0, count=0, kind=3 (Struct)
    bytes[448..450].copy_from_slice(&0u16.to_le_bytes()); // data (member index)
    bytes[450] = 0; // count
    bytes[451] = 3; // kind=Struct

    // TypeMembers sub-section at 512 (empty)
    // TypeNames sub-section at 576 (empty)

    // Entrypoints: one entry (name=StringId(0), target=StepId(0), result_type=QTypeId(0))
    let entrypoints_offset = 640;
    bytes[640..642].copy_from_slice(&0u16.to_le_bytes()); // name
    bytes[642..644].copy_from_slice(&0u16.to_le_bytes()); // target
    bytes[644..646].copy_from_slice(&0u16.to_le_bytes()); // result_type
    bytes[646..648].copy_from_slice(&0u16.to_le_bytes()); // padding

    // Transitions: one Match8 instruction (accept state)
    let transitions_offset = 704;
    // type_id=0x00 (Match8, segment 0)
    bytes[704] = 0x00;
    // nav=Stay
    bytes[705] = Nav::Stay.to_byte();
    // node_type=None (0)
    bytes[706..708].copy_from_slice(&0u16.to_le_bytes());
    // node_field=None (0)
    bytes[708..710].copy_from_slice(&0u16.to_le_bytes());
    // next=0 (accept)
    bytes[710..712].copy_from_slice(&0u16.to_le_bytes());

    // Build header
    let header = Header {
        magic: MAGIC,
        version: VERSION,
        checksum: 0,
        total_size: 768,
        str_blob_offset: str_blob_offset as u32,
        str_table_offset: str_table_offset as u32,
        node_types_offset: node_types_offset as u32,
        node_fields_offset: node_fields_offset as u32,
        trivia_offset: trivia_offset as u32,
        type_meta_offset: type_meta_offset as u32,
        entrypoints_offset: entrypoints_offset as u32,
        transitions_offset: transitions_offset as u32,
        str_table_count: 1,
        node_types_count: 1,
        node_fields_count: 1,
        trivia_count: 1,
        entrypoints_count: 1,
        transitions_count: 1,
        ..Default::default()
    };

    bytes[0..64].copy_from_slice(&header.to_bytes());
    bytes
}

#[test]
fn module_from_bytes_valid() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    assert!(module.header().validate_magic());
    assert!(module.header().validate_version());
    assert_eq!(module.header().total_size, 768);
}

#[test]
fn module_from_bytes_too_small() {
    let bytes = vec![0u8; 32];
    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(err, ModuleError::FileTooSmall(32)));
}

#[test]
fn module_from_bytes_invalid_magic() {
    let mut bytes = build_test_bytecode();
    bytes[0] = b'X'; // Corrupt magic
    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(err, ModuleError::InvalidMagic));
}

#[test]
fn module_from_bytes_wrong_version() {
    let mut bytes = build_test_bytecode();
    bytes[4..8].copy_from_slice(&999u32.to_le_bytes()); // Wrong version
    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(err, ModuleError::UnsupportedVersion(999)));
}

#[test]
fn module_from_bytes_size_mismatch() {
    let mut bytes = build_test_bytecode();
    bytes[12..16].copy_from_slice(&1000u32.to_le_bytes()); // Wrong total_size
    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(
        err,
        ModuleError::SizeMismatch {
            header: 1000,
            actual: 768
        }
    ));
}

#[test]
fn module_decode_step() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let instr = module.decode_step(StepId(0));
    match instr {
        Instruction::Match(m) => {
            assert_eq!(m.nav, Nav::Stay);
            assert!(m.is_epsilon());
            assert!(m.is_terminal());
        }
        _ => panic!("expected Match instruction"),
    }
}

#[test]
fn module_strings_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let strings = module.strings();
    assert_eq!(strings.get(StringId(0)), "Test");
}

#[test]
fn module_node_types_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let node_types = module.node_types();
    assert_eq!(node_types.len(), 1);
    assert!(!node_types.is_empty());

    let sym = node_types.get(0);
    assert_eq!(sym.id, 42);
    assert_eq!(sym.name, StringId(0));
}

#[test]
fn module_node_fields_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let fields = module.node_fields();
    assert_eq!(fields.len(), 1);

    let sym = fields.get(0);
    assert_eq!(sym.id, 7);
    assert_eq!(sym.name, StringId(0));
}

#[test]
fn module_trivia_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let trivia = module.trivia();
    assert_eq!(trivia.len(), 1);
    assert!(trivia.contains(100));
    assert!(!trivia.contains(42));
}

#[test]
fn module_types_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let types = module.types();
    assert_eq!(types.defs_count(), 1);
    assert_eq!(types.members_count(), 0);
    assert_eq!(types.names_count(), 0);

    let def = types.get_def(0);
    assert_eq!(def.kind, 3); // Struct
    assert_eq!(def.data, 0); // member index
    assert_eq!(def.count, 0); // member count
}

#[test]
fn module_entrypoints_view() {
    let bytes = build_test_bytecode();
    let module = Module::from_bytes(bytes).unwrap();

    let entrypoints = module.entrypoints();
    assert_eq!(entrypoints.len(), 1);
    assert!(!entrypoints.is_empty());

    let ep = entrypoints.get(0);
    assert_eq!(ep.name, StringId(0));
    assert_eq!(ep.target, StepId(0));

    let strings = module.strings();
    let found = entrypoints.find_by_name("Test", &strings);
    assert!(found.is_some());
    assert_eq!(found.unwrap().target, StepId(0));
}

#[test]
fn instruction_from_bytes_dispatch() {
    // Test Match8
    let match8 = Match {
        segment: 0,
        nav: Nav::Down,
        node_type: std::num::NonZeroU16::new(42),
        node_field: None,
        pre_effects: vec![],
        neg_fields: vec![],
        post_effects: vec![],
        successors: vec![StepId(10)],
    };
    let bytes = match8.to_bytes().unwrap();
    let instr = Instruction::from_bytes(&bytes);
    assert!(matches!(instr, Instruction::Match(_)));

    // Test Call
    let call = Call {
        segment: 0,
        next: StepId(5),
        target: StepId(100),
        ref_id: 1,
    };
    let bytes = call.to_bytes();
    let instr = Instruction::from_bytes(&bytes);
    assert!(matches!(instr, Instruction::Call(_)));

    // Test Return
    let ret = Return {
        segment: 0,
        ref_id: 1,
    };
    let bytes = ret.to_bytes();
    let instr = Instruction::from_bytes(&bytes);
    assert!(matches!(instr, Instruction::Return(_)));
}

#[test]
fn byte_storage_deref() {
    let data = vec![1, 2, 3, 4, 5];
    let storage = ByteStorage::from_vec(data.clone());

    assert_eq!(&*storage, &data[..]);
    assert_eq!(storage.len(), 5);
    assert_eq!(storage[2], 3);
}

#[test]
fn module_from_path_mmap() {
    use std::io::Write;

    let bytes = build_test_bytecode();

    // Write to temp file
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(&bytes).unwrap();
    tmpfile.flush().unwrap();

    // Load via mmap
    let module = Module::from_path(tmpfile.path()).unwrap();

    assert!(module.header().validate_magic());
    assert_eq!(module.header().total_size, 768);

    // Verify we can decode instructions
    let instr = module.decode_step(StepId(0));
    assert!(matches!(instr, Instruction::Match(_)));

    // Verify string lookup works through mmap
    let strings = module.strings();
    assert_eq!(strings.get(StringId(0)), "Test");
}
