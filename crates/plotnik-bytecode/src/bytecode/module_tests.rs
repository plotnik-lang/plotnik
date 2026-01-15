//! Tests for the bytecode module.

use super::module::{ByteStorage, ModuleError};
use super::AlignedVec;

#[test]
fn byte_storage_copy_from_slice() {
    let data = [1u8, 2, 3, 4, 5];
    let storage = ByteStorage::copy_from_slice(&data);

    assert_eq!(&*storage, &data[..]);
    assert_eq!(storage.len(), 5);
    assert_eq!(storage[2], 3);
}

#[test]
fn byte_storage_from_aligned() {
    let vec = AlignedVec::copy_from_slice(&[1, 2, 3, 4, 5]);
    let storage = ByteStorage::from_aligned(vec);

    assert_eq!(&*storage, &[1, 2, 3, 4, 5]);
    assert_eq!(storage.len(), 5);
}

#[test]
fn module_error_display() {
    let err = ModuleError::InvalidMagic;
    assert_eq!(err.to_string(), "invalid magic: expected PTKQ");

    let err = ModuleError::UnsupportedVersion(99);
    assert!(err.to_string().contains("99"));

    let err = ModuleError::FileTooSmall(32);
    assert!(err.to_string().contains("32"));

    let err = ModuleError::SizeMismatch {
        header: 100,
        actual: 50,
    };
    assert!(err.to_string().contains("100"));
    assert!(err.to_string().contains("50"));
}
