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
}
