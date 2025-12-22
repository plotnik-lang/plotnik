//! Symbol interning for field and type names.
//!
//! Converts heap-allocated strings into cheap integer handles.
//! Comparing two symbols is O(1) integer comparison.

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
}

// Implement Ord based on raw index (insertion order).
// For deterministic output, sort by resolved string when needed.
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

        // Symbol is Copy, so this should work without move
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
}
