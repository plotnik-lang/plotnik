//! String interning for efficient string deduplication and comparison.
//!
//! Converts heap-allocated strings into cheap integer handles (`Symbol`).
//! Comparing two symbols is O(1) integer comparison.
//!
//! The interner can be serialized to a binary blob format for the compiled query.

use std::collections::HashMap;

/// A lightweight handle to an interned string.
///
/// Comparing two symbols is O(1). Symbols are ordered by insertion order,
/// not lexicographically—use `Interner::resolve` if you need string ordering.
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
    /// Map from string to symbol for deduplication.
    map: HashMap<String, Symbol>,
    /// Storage for interned strings, indexed by Symbol.
    strings: Vec<String>,
}

impl Interner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Intern a string, returning its Symbol.
    /// If the string was already interned, returns the existing Symbol.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }

        let sym = Symbol(self.strings.len() as u32);
        self.strings.push(s.to_owned());
        self.map.insert(s.to_owned(), sym);
        sym
    }

    /// Intern an owned string, avoiding clone if not already present.
    pub fn intern_owned(&mut self, s: String) -> Symbol {
        if let Some(&sym) = self.map.get(&s) {
            return sym;
        }

        let sym = Symbol(self.strings.len() as u32);
        self.strings.push(s.clone());
        self.map.insert(s, sym);
        sym
    }

    /// Resolve a Symbol back to its string.
    ///
    /// # Panics
    /// Panics if the symbol was not created by this interner.
    #[inline]
    pub fn resolve(&self, sym: Symbol) -> &str {
        &self.strings[sym.0 as usize]
    }

    /// Try to resolve a Symbol, returning None if invalid.
    #[inline]
    pub fn try_resolve(&self, sym: Symbol) -> Option<&str> {
        self.strings.get(sym.0 as usize).map(|s| s.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplicates() {
        let mut interner = Interner::new();

        let a = interner.intern("foo");
        let b = interner.intern("foo");
        let c = interner.intern("bar");

        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn resolve_roundtrip() {
        let mut interner = Interner::new();

        let sym = interner.intern("hello");
        assert_eq!(interner.resolve(sym), "hello");
    }

    #[test]
    fn intern_owned_avoids_clone_on_hit() {
        let mut interner = Interner::new();

        let a = interner.intern("test");
        let b = interner.intern_owned("test".to_string());

        assert_eq!(a, b);
        assert_eq!(interner.len(), 1);
    }

    #[test]
    fn symbols_are_copy() {
        let mut interner = Interner::new();
        let sym = interner.intern("x");

        let copy = sym;
        assert_eq!(sym, copy);
    }

    #[test]
    fn symbol_ordering_is_insertion_order() {
        let mut interner = Interner::new();

        let z = interner.intern("z");
        let a = interner.intern("a");

        // z was inserted first, so z < a by insertion order
        assert!(z < a);
    }

    #[test]
    fn to_blob_produces_correct_format() {
        let mut interner = Interner::new();
        interner.intern("id");
        interner.intern("foo");

        let (blob, offsets) = interner.to_blob();

        assert_eq!(blob, b"idfoo");
        assert_eq!(offsets, vec![0, 2, 5]);

        // Verify we can reconstruct strings
        let s0 = &blob[offsets[0] as usize..offsets[1] as usize];
        let s1 = &blob[offsets[1] as usize..offsets[2] as usize];
        assert_eq!(s0, b"id");
        assert_eq!(s1, b"foo");
    }

    #[test]
    fn to_blob_empty() {
        let interner = Interner::new();
        let (blob, offsets) = interner.to_blob();

        assert!(blob.is_empty());
        assert_eq!(offsets, vec![0]); // just the sentinel
    }

    #[test]
    fn iter_yields_all_strings() {
        let mut interner = Interner::new();
        let a = interner.intern("alpha");
        let b = interner.intern("beta");

        let items: Vec<_> = interner.iter().collect();
        assert_eq!(items, vec![(a, "alpha"), (b, "beta")]);
    }

    #[test]
    fn symbol_from_raw_roundtrip() {
        let sym = Symbol::from_raw(42);
        assert_eq!(sym.as_u32(), 42);
    }
}
