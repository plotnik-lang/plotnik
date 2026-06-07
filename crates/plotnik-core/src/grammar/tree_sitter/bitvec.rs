use std::{
    hash::{Hash, Hasher},
    ops::Index,
};

use fixedbitset::{Block, FixedBitSet};

const BITS_PER_BLOCK: usize = Block::BITS as usize;

pub type BitBlock = Block;

/// Compatibility adapter for Tree-sitter's token-set bit vector semantics.
///
/// `FixedBitSet` includes logical length in equality, hashing, and ordering.
/// Tree-sitter token sets instead compare the set bits only, ignoring trailing
/// zero words. Keep those semantics here so the vendored grammar lowering code
/// behaves like upstream.
#[derive(Clone, Debug)]
pub struct BitVec {
    bits: FixedBitSet,
    len: usize,
}

impl BitVec {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bits: FixedBitSet::new(),
            len: 0,
        }
    }

    #[must_use]
    pub fn with_capacity(n_bits: usize) -> Self {
        Self {
            bits: FixedBitSet::with_capacity(n_bits),
            len: 0,
        }
    }

    #[inline]
    const fn words_in_use(&self) -> usize {
        self.len.div_ceil(BITS_PER_BLOCK)
    }

    /// View the in-use words as a slice.
    #[inline]
    #[must_use]
    pub fn as_slice(&self) -> &[Block] {
        &self.bits.as_slice()[..self.words_in_use()]
    }

    #[inline]
    fn as_full_slice_mut(&mut self) -> &mut [Block] {
        self.bits.as_mut_slice()
    }

    #[must_use]
    #[allow(clippy::len_without_is_empty)]
    pub const fn len(&self) -> usize {
        self.len
    }

    #[must_use]
    pub fn get(&self, index: usize) -> Option<bool> {
        if index >= self.len {
            return None;
        }

        Some(self.bits.contains(index))
    }

    pub fn set(&mut self, index: usize, val: bool) {
        assert!(
            index < self.len,
            "cannot set bit {index} in bit vector of length {}",
            self.len
        );

        if val {
            self.bits.insert(index);
        } else {
            self.bits.remove(index);
        }
    }

    pub fn resize(&mut self, new_len: usize, val: bool) {
        if new_len > self.bits.len() {
            self.bits.grow(new_len);
        }

        if new_len > self.len && val {
            for bit in self.len..new_len {
                self.bits.insert(bit);
            }
        }

        if new_len < self.len {
            for bit in new_len..self.len {
                self.bits.remove(bit);
            }
        }

        self.len = new_len;
    }

    #[must_use]
    pub fn last(&self) -> Option<bool> {
        if self.len == 0 {
            return None;
        }

        self.get(self.len - 1)
    }

    pub fn pop(&mut self) -> Option<bool> {
        if self.len == 0 {
            return None;
        }

        let index = self.len - 1;
        let val = self.bits.contains(index);
        self.bits.remove(index);
        self.len = index;
        Some(val)
    }

    /// Word-level OR: self |= other. Returns true if any new bits were set.
    #[inline]
    pub fn insert_all(&mut self, other: &Self) -> bool {
        let other_words = other.words_in_use();
        if other_words == 0 {
            return false;
        }

        if other.len > self.bits.len() {
            self.bits.grow(other.len);
        }

        if other.len > self.len {
            self.len = other.len;
        }

        let other_slice = other.as_slice();
        let self_slice = &mut self.as_full_slice_mut()[..other_words];
        let mut any_new = 0;

        for (self_word, &other_word) in self_slice.iter_mut().zip(other_slice) {
            let new_bits = other_word & !*self_word;
            *self_word |= other_word;
            any_new |= new_bits;
        }

        any_new != 0
    }
}

impl Default for BitVec {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for BitVec {
    fn eq(&self, other: &Self) -> bool {
        let a = self.as_slice();
        let b = other.as_slice();
        let max_len = a.len().max(b.len());
        for i in 0..max_len {
            if a.get(i).copied().unwrap_or(0) != b.get(i).copied().unwrap_or(0) {
                return false;
            }
        }
        true
    }
}

impl Eq for BitVec {}

impl Hash for BitVec {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let data = self.as_slice();
        let effective_len = data.iter().rposition(|&w| w != 0).map_or(0, |i| i + 1);
        data[..effective_len].hash(state);
    }
}

impl Ord for BitVec {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let a = self.as_slice();
        let b = other.as_slice();
        let max_len = a.len().max(b.len());
        for i in 0..max_len {
            let aw = a.get(i).copied().unwrap_or(0);
            let bw = b.get(i).copied().unwrap_or(0);
            if aw != bw {
                let first_diff = (aw ^ bw).trailing_zeros();
                return if (aw >> first_diff) & 1 != 0 {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                };
            }
        }
        std::cmp::Ordering::Equal
    }
}

impl PartialOrd for BitVec {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Index<usize> for BitVec {
    type Output = bool;

    fn index(&self, index: usize) -> &Self::Output {
        static TRUE: bool = true;
        static FALSE: bool = false;
        if self.bits.contains(index) {
            &TRUE
        } else {
            &FALSE
        }
    }
}

/// Iterator that yields only the indices of set bits, skipping zero words
/// entirely and using `trailing_zeros()` within each word.
pub struct SetBitsIter<'a> {
    data: &'a [Block],
    word_idx: usize,
    current_word: Block,
}

impl<'a> SetBitsIter<'a> {
    #[must_use]
    pub fn new(data: &'a [Block]) -> Self {
        Self {
            data,
            word_idx: 0,
            current_word: data.first().copied().unwrap_or(0),
        }
    }
}

impl Iterator for SetBitsIter<'_> {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        while self.current_word == 0 {
            self.word_idx += 1;
            if self.word_idx >= self.data.len() {
                return None;
            }
            self.current_word = self.data[self.word_idx];
        }
        let bit = self.current_word.trailing_zeros() as usize;
        self.current_word &= self.current_word - 1;
        Some(self.word_idx * BITS_PER_BLOCK + bit)
    }
}
