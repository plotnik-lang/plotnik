//! Relative range within a segment.
//!
//! `start_index` is an **element index**, not a byte offset. This naming
//! distinguishes it from byte offsets like `StringRef.offset`.
//!
//! This struct is 8 bytes with 4-byte alignment for efficient access.
//! Type safety is provided through generic methods, not stored PhantomData.

use std::marker::PhantomData;

/// Relative range within a compiled query segment.
///
/// Used for variable-length data (successors, effects, negated fields, type members).
/// The slice references elements by index into the corresponding segment array.
///
/// Layout: 8 bytes (4 + 2 + 2), align 4.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Slice<T> {
    /// Element index into the segment array (NOT byte offset).
    start_index: u32,
    /// Number of elements. 65k elements per slice is sufficient.
    len: u16,
    _pad: u16,
    _phantom: PhantomData<fn() -> T>,
}

// Compile-time size/alignment verification
const _: () = assert!(size_of::<Slice<u8>>() == 8);
const _: () = assert!(align_of::<Slice<u8>>() == 4);

impl<T> Slice<T> {
    /// Creates a new slice.
    #[inline]
    pub const fn new(start_index: u32, len: u16) -> Self {
        Self {
            start_index,
            len,
            _pad: 0,
            _phantom: PhantomData,
        }
    }

    /// Creates an empty slice.
    #[inline]
    pub const fn empty() -> Self {
        Self::new(0, 0)
    }

    /// Returns the start index (element index, not byte offset).
    #[inline]
    pub fn start_index(&self) -> u32 {
        self.start_index
    }

    /// Returns the number of elements.
    #[inline]
    pub fn len(&self) -> u16 {
        self.len
    }

    /// Returns true if the slice is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Creates a slice encoding an inner type ID (for wrapper TypeDef).
    /// The `start_index` stores the TypeId as u32, `len` is 0.
    #[inline]
    pub const fn from_inner_type(type_id: u16) -> Self {
        Self::new(type_id as u32, 0)
    }
}

impl<T> Default for Slice<T> {
    fn default() -> Self {
        Self::empty()
    }
}

impl<T> PartialEq for Slice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.start_index == other.start_index && self.len == other.len
    }
}

impl<T> Eq for Slice<T> {}

impl<T> std::fmt::Debug for Slice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Slice")
            .field("start_index", &self.start_index)
            .field("len", &self.len)
            .finish()
    }
}
