//! String interning for efficient string deduplication and comparison.
//!
//! Converts heap-allocated strings into cheap integer handles (`Symbol`).
//! Comparing two symbols is O(1) integer comparison.
//!
//! The interner can be serialized to a binary blob format for the compiled query.

use indexmap::IndexSet;
use rustc_hash::FxBuildHasher;

/// A lightweight handle to an interned string.
///
/// Comparing two symbols is O(1). Symbols are ordered by insertion order,
/// not lexicographically — use `Interner::resolve` if you need string ordering.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Symbol(u32);

impl Symbol {
    /// Raw index for serialization/debugging.
    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }

    /// Create a Symbol from a raw index. Use only for deserialization.
    #[inline]
    pub fn from_raw(index: u32) -> Self {
        Self(index)
    }
}

impl PartialOrd for Symbol {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Symbol {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0)
    }
}

/// String interner. Deduplicates strings and returns cheap Symbol handles.
#[derive(Debug, Clone, Default)]
pub struct Interner {
    /// Interned strings in insertion order; each string's index is its `Symbol`, and the set
    /// doubles as the dedup lookup. Strings are trusted internal identifiers, never adversarial
    /// input, so a non-cryptographic hasher is fine.
    strings: IndexSet<String, FxBuildHasher>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a string, returning its Symbol.
    /// If the string was already interned, returns the existing Symbol.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(idx) = self.strings.get_index_of(s) {
            return Symbol(idx as u32);
        }

        let (idx, _) = self.strings.insert_full(s.to_owned());
        Symbol(idx as u32)
    }

    /// Return the symbol for an already-interned string.
    pub fn get(&self, s: &str) -> Option<Symbol> {
        self.strings.get_index_of(s).map(|i| Symbol(i as u32))
    }

    /// Resolve a Symbol back to its string.
    ///
    /// # Panics
    /// Panics if the symbol was not created by this interner.
    #[inline]
    pub fn resolve(&self, sym: Symbol) -> &str {
        self.strings
            .get_index(sym.0 as usize)
            .expect("symbol was not created by this interner")
    }

    /// Try to resolve a Symbol, returning None if invalid.
    #[inline]
    pub fn try_resolve(&self, sym: Symbol) -> Option<&str> {
        self.strings.get_index(sym.0 as usize).map(String::as_str)
    }

    /// Number of interned strings.
    #[inline]
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Whether the interner is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Iterate over all interned strings with their symbols.
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = (Symbol, &str)> {
        self.strings
            .iter()
            .enumerate()
            .map(|(i, s)| (Symbol(i as u32), s.as_str()))
    }

    /// Emit as binary format blob and offset table.
    ///
    /// Returns (concatenated UTF-8 bytes, offset for each string + sentinel).
    /// The offsets array has `len() + 1` entries; the last is the total blob size.
    pub fn to_blob(&self) -> (Vec<u8>, Vec<u32>) {
        let mut blob = Vec::new();
        let mut offsets = Vec::with_capacity(self.strings.len() + 1);

        for s in &self.strings {
            offsets.push(blob.len() as u32);
            blob.extend_from_slice(s.as_bytes());
        }
        offsets.push(blob.len() as u32); // sentinel for length calculation

        (blob, offsets)
    }
}
