//! Unit tests for TypeTableBuilder.

use super::type_table::TypeTableBuilder;

#[test]
fn new_builder_is_empty() {
    let builder = TypeTableBuilder::new();

    assert_eq!(builder.type_defs_count(), 0);
    assert_eq!(builder.type_members_count(), 0);
    assert_eq!(builder.type_names_count(), 0);
}

#[test]
fn validate_passes_for_empty_builder() {
    let builder = TypeTableBuilder::new();

    assert!(builder.validate().is_ok());
}

#[test]
fn emit_empty_builder_produces_empty_sections() {
    let builder = TypeTableBuilder::new();

    let (defs, members, names) = builder.emit();

    assert!(defs.is_empty());
    assert!(members.is_empty());
    assert!(names.is_empty());
}
