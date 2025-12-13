//! String pool entry reference.
//!
//! Strings are stored in a single contiguous byte pool. `StringRef` points
//! into that pool via byte offset (not element index).

/// Reference to a string in the string pool.
///
/// Layout: 8 bytes, align 4.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StringRef {
    /// Byte offset into string_bytes segment.
    pub offset: u32,
    /// Length of the string in bytes.
    pub len: u16,
    _pad: u16,
}

impl StringRef {
    pub const fn new(offset: u32, len: u16) -> Self {
        Self {
            offset,
            len,
            _pad: 0,
        }
    }
}

// Compile-time size verification
const _: () = assert!(size_of::<StringRef>() == 8);
const _: () = assert!(align_of::<StringRef>() == 4);
