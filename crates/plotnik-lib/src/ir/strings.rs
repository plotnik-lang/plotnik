//! String interning for compiled queries.
//!
//! Identical strings share storage and ID. Used for field names, variant tags,
//! entrypoint names, and type names.

use std::collections::HashMap;

use super::ids::StringId;

/// String interner for query compilation.
///
/// Interns strings during the analysis phase, then emits them as a contiguous
/// byte pool with `StringRef` entries pointing into it.
#[derive(Debug, Default)]
pub struct StringInterner<'src> {
    /// Map from string content to assigned ID.
    map: HashMap<&'src str, StringId>,
    /// Strings in ID order for emission.
    strings: Vec<&'src str>,
}

impl<'src> StringInterner<'src> {
    /// Creates a new empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Interns a string, returning its ID.
    ///
    /// If the string was previously interned, returns the existing ID.
    pub fn intern(&mut self, s: &'src str) -> StringId {
        if let Some(&id) = self.map.get(s) {
            return id;
        }

        let id = self.strings.len() as StringId;
        assert!(id < 0xFFFF, "string pool overflow (>65534 strings)");

        self.map.insert(s, id);
        self.strings.push(s);
        id
    }

    /// Returns the ID of a previously interned string, or `None`.
    pub fn get(&self, s: &str) -> Option<StringId> {
        self.map.get(s).copied()
    }

    /// Returns the string for a given ID.
    ///
    /// # Panics
    /// Panics if the ID is out of range.
    pub fn resolve(&self, id: StringId) -> &'src str {
        self.strings[id as usize]
    }

    /// Returns the number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Returns true if no strings have been interned.
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// Returns an iterator over (id, string) pairs in ID order.
    pub fn iter(&self) -> impl Iterator<Item = (StringId, &'src str)> + '_ {
        self.strings
            .iter()
            .enumerate()
            .map(|(i, s)| (i as StringId, *s))
    }

    /// Returns the total byte size needed for all strings.
    pub fn total_bytes(&self) -> usize {
        self.strings.iter().map(|s| s.len()).sum()
    }

    /// Consumes the interner and returns strings in ID order.
    pub fn into_strings(self) -> Vec<&'src str> {
        self.strings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intern_deduplicates() {
        let mut interner = StringInterner::new();

        let id1 = interner.intern("foo");
        let id2 = interner.intern("bar");
        let id3 = interner.intern("foo");

        assert_eq!(id1, 0);
        assert_eq!(id2, 1);
        assert_eq!(id3, 0); // same as id1
        assert_eq!(interner.len(), 2);
    }

    #[test]
    fn resolve_works() {
        let mut interner = StringInterner::new();
        interner.intern("hello");
        interner.intern("world");

        assert_eq!(interner.resolve(0), "hello");
        assert_eq!(interner.resolve(1), "world");
    }

    #[test]
    fn get_returns_none_for_unknown() {
        let interner = StringInterner::new();
        assert_eq!(interner.get("unknown"), None);
    }

    #[test]
    fn total_bytes() {
        let mut interner = StringInterner::new();
        interner.intern("foo"); // 3 bytes
        interner.intern("hello"); // 5 bytes
        interner.intern("foo"); // deduplicated

        assert_eq!(interner.total_bytes(), 8);
    }

    #[test]
    fn iter_order() {
        let mut interner = StringInterner::new();
        interner.intern("a");
        interner.intern("b");
        interner.intern("c");

        let pairs: Vec<_> = interner.iter().collect();
        assert_eq!(pairs, vec![(0, "a"), (1, "b"), (2, "c")]);
    }
}
