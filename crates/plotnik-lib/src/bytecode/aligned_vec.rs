//! 64-byte aligned storage for bytecode.
//!
//! Bytecode sections are 64-byte aligned internally. For this alignment to be
//! meaningful at runtime, the buffer itself must start at a 64-byte boundary.
//! Standard `Vec<u8>` provides no alignment guarantees for `u8`.

use std::ops::Deref;

use super::SECTION_ALIGN;

/// `repr(align(..))` only accepts a literal, so the alignment is spelled out
/// here; this guards it against ever drifting from `SECTION_ALIGN`.
const _: () = assert!(SECTION_ALIGN == 64);

/// One section-aligned block of bytecode storage.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Block([u8; SECTION_ALIGN]);

/// Immutable 64-byte aligned byte storage.
///
/// Uses `Vec<Block>` internally — Vec guarantees element alignment,
/// so the data starts at a 64-byte boundary. No custom allocator needed.
pub struct AlignedVec {
    blocks: Vec<Block>,
    len: usize,
}

impl AlignedVec {
    /// Copy bytes into aligned storage.
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        if bytes.is_empty() {
            return Self {
                blocks: Vec::new(),
                len: 0,
            };
        }

        let num_blocks = bytes.len().div_ceil(SECTION_ALIGN);
        let mut blocks = vec![Block([0; SECTION_ALIGN]); num_blocks];

        for (i, chunk) in bytes.chunks(SECTION_ALIGN).enumerate() {
            blocks[i].0[..chunk.len()].copy_from_slice(chunk);
        }

        Self {
            blocks,
            len: bytes.len(),
        }
    }

    /// Number of bytes stored.
    pub fn len(&self) -> usize {
        self.len
    }

    /// View as byte slice.
    pub fn as_slice(&self) -> &[u8] {
        if self.blocks.is_empty() {
            return &[];
        }
        if self.len > self.blocks.len() * SECTION_ALIGN {
            panic!(
                "AlignedVec invariant violated: len {} exceeds capacity {}",
                self.len,
                self.blocks.len() * SECTION_ALIGN
            );
        }
        // SAFETY: Block is repr(C) with only [u8; 64], so pointer cast is valid.
        // We only expose `len` bytes, which were initialized in copy_from_slice.
        unsafe { std::slice::from_raw_parts(self.blocks.as_ptr() as *const u8, self.len) }
    }
}

impl Deref for AlignedVec {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Clone for AlignedVec {
    fn clone(&self) -> Self {
        Self {
            blocks: self.blocks.clone(),
            len: self.len,
        }
    }
}

impl std::fmt::Debug for AlignedVec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AlignedVec")
            .field("len", &self.len)
            .field(
                "aligned",
                &(self.blocks.as_ptr() as usize).is_multiple_of(SECTION_ALIGN),
            )
            .finish()
    }
}
