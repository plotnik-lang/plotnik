//! 64-byte aligned storage for bytecode.
//!
//! Bytecode sections are 64-byte aligned internally. For this alignment to be
//! meaningful at runtime, the buffer itself must start at a 64-byte boundary.
//! Standard `Vec<u8>` provides no alignment guarantees for `u8`.

use std::ops::Deref;

/// Alignment for bytecode buffers (matches `SECTION_ALIGN`).
pub const ALIGN: usize = 64;

/// 64-byte aligned block for bytecode storage.
#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Block([u8; 64]);

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

        let num_blocks = bytes.len().div_ceil(64);
        let mut blocks = vec![Block([0; 64]); num_blocks];

        // Copy block by block to stay safe
        for (i, chunk) in bytes.chunks(64).enumerate() {
            blocks[i].0[..chunk.len()].copy_from_slice(chunk);
        }

        Self {
            blocks,
            len: bytes.len(),
        }
    }

    /// Read a file into aligned storage.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self::copy_from_slice(&bytes))
    }

    /// Number of bytes stored.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// View as byte slice.
    pub fn as_slice(&self) -> &[u8] {
        if self.blocks.is_empty() {
            return &[];
        }
        if self.len > self.blocks.len() * 64 {
            panic!(
                "AlignedVec invariant violated: len {} exceeds capacity {}",
                self.len,
                self.blocks.len() * 64
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
                &(self.blocks.as_ptr() as usize).is_multiple_of(ALIGN),
            )
            .finish()
    }
}
