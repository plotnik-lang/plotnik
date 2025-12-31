//! Tests for the bytecode module.

use indoc::indoc;

use crate::Query;
use crate::bytecode::{Module, ModuleError, StepId, StringId};

#[test]
fn module_from_bytes_valid() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    assert!(module.header().validate_magic());
    assert!(module.header().validate_version());
}

#[test]
fn module_from_bytes_too_small() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let truncated = bytes[..32].to_vec();

    let err = Module::from_bytes(truncated).unwrap_err();
    assert!(matches!(err, ModuleError::FileTooSmall(32)));
}

#[test]
fn module_from_bytes_invalid_magic() {
    let input = "Test = (identifier) @id";

    let mut bytes = Query::expect_valid_linked_bytes(input);
    bytes[0] = b'X'; // Corrupt magic

    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(err, ModuleError::InvalidMagic));
}

#[test]
fn module_from_bytes_wrong_version() {
    let input = "Test = (identifier) @id";

    let mut bytes = Query::expect_valid_linked_bytes(input);
    bytes[4..8].copy_from_slice(&999u32.to_le_bytes()); // Wrong version

    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(err, ModuleError::UnsupportedVersion(999)));
}

#[test]
fn module_from_bytes_size_mismatch() {
    let input = "Test = (identifier) @id";

    let mut bytes = Query::expect_valid_linked_bytes(input);
    let actual_size = bytes.len() as u32;
    bytes[12..16].copy_from_slice(&(actual_size + 100).to_le_bytes()); // Wrong total_size

    let err = Module::from_bytes(bytes).unwrap_err();
    assert!(matches!(
        err,
        ModuleError::SizeMismatch {
            header: h,
            actual: a
        } if h == actual_size + 100 && a == actual_size as usize
    ));
}

#[test]
fn module_strings_view() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    let strings = module.strings();
    // String 0 is the easter egg
    assert_eq!(strings.get(StringId(0)), "Beauty will save the world");
    // Other strings include "id", "Test", "identifier"
    assert!(module.header().str_table_count >= 3);
}

#[test]
fn module_node_types_view() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    let node_types = module.node_types();
    assert!(!node_types.is_empty());
    // Should have "identifier" node type
    let has_identifier = (0..node_types.len()).any(|i| {
        let sym = node_types.get(i);
        module.strings().get(sym.name) == "identifier"
    });
    assert!(has_identifier);
}

#[test]
fn module_node_fields_view() {
    let input = "Test = (function_declaration name: (identifier) @name)";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    let fields = module.node_fields();
    assert!(!fields.is_empty());
    // Should have "name" field
    let has_name = (0..fields.len()).any(|i| {
        let sym = fields.get(i);
        module.strings().get(sym.name) == "name"
    });
    assert!(has_name);
}

#[test]
fn module_types_view() {
    let input = indoc! {r#"
        Test = (function_declaration
            name: (identifier) @name
            body: (_) @body)
    "#};

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    let types = module.types();
    // Should have custom types (struct with fields)
    assert!(types.defs_count() >= 1);
    assert!(types.members_count() >= 2); // name and body fields
}

#[test]
fn module_entrypoints_view() {
    let input = indoc! {r#"
        Foo = (identifier) @id
        Bar = (string) @str
    "#};

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    let entrypoints = module.entrypoints();
    assert_eq!(entrypoints.len(), 2);
    assert!(!entrypoints.is_empty());

    // Should be able to find by name
    let strings = module.strings();
    let foo = entrypoints.find_by_name("Foo", &strings);
    let bar = entrypoints.find_by_name("Bar", &strings);
    assert!(foo.is_some());
    assert!(bar.is_some());
}

#[test]
fn module_decode_step() {
    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);
    let module = Module::from_bytes(bytes).unwrap();

    // Step 0 is always the accept state (epsilon terminal)
    let instr = module.decode_step(StepId(0));
    match instr {
        crate::bytecode::Instruction::Match(m) => {
            assert!(m.is_epsilon());
            assert!(m.is_terminal());
        }
        _ => panic!("expected Match instruction at step 0"),
    }
}

#[test]
fn module_from_path_mmap() {
    use std::io::Write;

    let input = "Test = (identifier) @id";

    let bytes = Query::expect_valid_linked_bytes(input);

    // Write to temp file
    let mut tmpfile = tempfile::NamedTempFile::new().unwrap();
    tmpfile.write_all(&bytes).unwrap();
    tmpfile.flush().unwrap();

    // Load via mmap
    let module = Module::from_path(tmpfile.path()).unwrap();

    assert!(module.header().validate_magic());

    // Verify we can decode instructions
    let instr = module.decode_step(StepId(0));
    assert!(matches!(instr, crate::bytecode::Instruction::Match(_)));

    // Verify string lookup works through mmap
    let strings = module.strings();
    assert_eq!(strings.get(StringId(0)), "Beauty will save the world");
}

#[test]
fn byte_storage_deref() {
    use crate::bytecode::ByteStorage;

    let data = vec![1, 2, 3, 4, 5];
    let storage = ByteStorage::from_vec(data.clone());

    assert_eq!(&*storage, &data[..]);
    assert_eq!(storage.len(), 5);
    assert_eq!(storage[2], 3);
}
