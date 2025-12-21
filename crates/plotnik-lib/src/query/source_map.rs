//! Arena-based source storage for unified lifetimes.
//!
//! All sources are stored in a single contiguous buffer, allowing all string slices
//! to share the same lifetime as `&SourceMap`. This eliminates lifetime complexity
//! when multiple sources need to be analyzed together.

use std::ops::Range;

/// Lightweight handle to a source in a compilation session.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Default)]
pub struct SourceId(u32);

/// Describes the origin of a source.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum SourceKind<'a> {
    /// A one-liner query passed directly (e.g., CLI `-q` argument).
    OneLiner,
    /// Input read from stdin.
    Stdin,
    /// A file with its path.
    File(&'a str),
}

impl SourceKind<'_> {
    /// Returns the display name for diagnostics.
    pub fn display_name(&self) -> &str {
        match self {
            SourceKind::OneLiner => "<query>",
            SourceKind::Stdin => "<stdin>",
            SourceKind::File(path) => path,
        }
    }
}

/// A borrowed view of a source: id, kind, and content.
/// All string slices share the lifetime of the `SourceMap`.
#[derive(Copy, Clone, Debug)]
pub struct Source<'a> {
    pub id: SourceId,
    pub kind: SourceKind<'a>,
    pub content: &'a str,
}

impl<'a> Source<'a> {
    /// Returns the content string.
    pub fn as_str(&self) -> &'a str {
        self.content
    }
}

/// Internal representation of source kind, storing ranges instead of slices.
#[derive(Clone, Debug)]
enum SourceKindEntry {
    OneLiner,
    Stdin,
    /// Stores the byte range of the filename in the shared buffer.
    File {
        name_range: Range<u32>,
    },
}

/// Metadata for a source in the arena.
#[derive(Clone, Debug)]
struct SourceEntry {
    kind: SourceKindEntry,
    /// Byte range of content in the shared buffer.
    content_range: Range<u32>,
}

/// Arena-based registry of all sources. Owns a single buffer.
///
/// All content slices returned have the same lifetime as `&SourceMap`,
/// eliminating the need for separate lifetimes per source file.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    buffer: String,
    entries: Vec<SourceEntry>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a one-liner source (CLI `-q` argument, REPL, tests).
    pub fn add_one_liner(&mut self, content: &str) -> SourceId {
        let content_range = self.push_content(content);
        self.push_entry(SourceKindEntry::OneLiner, content_range)
    }

    /// Add a source read from stdin.
    pub fn add_stdin(&mut self, content: &str) -> SourceId {
        let content_range = self.push_content(content);
        self.push_entry(SourceKindEntry::Stdin, content_range)
    }

    /// Add a file source with its path.
    pub fn add_file(&mut self, path: &str, content: &str) -> SourceId {
        let name_start = self.buffer.len() as u32;
        self.buffer.push_str(path);
        let name_end = self.buffer.len() as u32;

        let content_range = self.push_content(content);

        self.push_entry(
            SourceKindEntry::File {
                name_range: name_start..name_end,
            },
            content_range,
        )
    }

    /// Create a SourceMap with a single one-liner source.
    /// Convenience for single-source use cases (CLI, REPL, tests).
    pub fn one_liner(content: &str) -> Self {
        let mut map = Self::new();
        map.add_one_liner(content);
        map
    }

    /// Get the content of a source by ID.
    pub fn content(&self, id: SourceId) -> &str {
        self.entries
            .get(id.0 as usize)
            .map(|e| self.slice(&e.content_range))
            .expect("invalid SourceId")
    }

    /// Get the kind of a source by ID.
    pub fn kind(&self, id: SourceId) -> SourceKind<'_> {
        self.entries
            .get(id.0 as usize)
            .map(|e| self.resolve_kind(&e.kind))
            .expect("invalid SourceId")
    }

    /// Get the file path if this source is a file, None otherwise.
    pub fn path(&self, id: SourceId) -> Option<&str> {
        let entry = self.entries.get(id.0 as usize).expect("invalid SourceId");
        match &entry.kind {
            SourceKindEntry::File { name_range } => Some(self.slice(name_range)),
            _ => None,
        }
    }

    /// Number of sources in the map.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Get a source by ID, returning a `Source` view.
    pub fn get(&self, id: SourceId) -> Source<'_> {
        let entry = self.entries.get(id.0 as usize).expect("invalid SourceId");
        Source {
            id,
            kind: self.resolve_kind(&entry.kind),
            content: self.slice(&entry.content_range),
        }
    }

    /// Iterate over all sources as `Source` views.
    pub fn iter(&self) -> impl Iterator<Item = Source<'_>> {
        self.entries.iter().enumerate().map(|(idx, entry)| Source {
            id: SourceId(idx as u32),
            kind: self.resolve_kind(&entry.kind),
            content: self.slice(&entry.content_range),
        })
    }

    /// Reserve additional capacity in the buffer.
    /// Useful when loading multiple files to avoid reallocations.
    pub fn reserve(&mut self, additional: usize) {
        self.buffer.reserve(additional);
    }

    fn push_content(&mut self, content: &str) -> Range<u32> {
        let start = self.buffer.len() as u32;
        self.buffer.push_str(content);
        let end = self.buffer.len() as u32;
        start..end
    }

    fn push_entry(&mut self, kind: SourceKindEntry, content_range: Range<u32>) -> SourceId {
        let id = SourceId(self.entries.len() as u32);
        self.entries.push(SourceEntry {
            kind,
            content_range,
        });
        id
    }

    fn slice(&self, range: &Range<u32>) -> &str {
        &self.buffer[range.start as usize..range.end as usize]
    }

    fn resolve_kind(&self, kind: &SourceKindEntry) -> SourceKind<'_> {
        match kind {
            SourceKindEntry::OneLiner => SourceKind::OneLiner,
            SourceKindEntry::Stdin => SourceKind::Stdin,
            SourceKindEntry::File { name_range } => SourceKind::File(self.slice(name_range)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_one_liner() {
        let map = SourceMap::one_liner("hello world");
        let id = SourceId(0);

        assert_eq!(map.content(id), "hello world");
        assert_eq!(map.kind(id), SourceKind::OneLiner);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn stdin_source() {
        let mut map = SourceMap::new();
        let id = map.add_stdin("from stdin");

        assert_eq!(map.content(id), "from stdin");
        assert_eq!(map.kind(id), SourceKind::Stdin);
    }

    #[test]
    fn file_source() {
        let mut map = SourceMap::new();
        let id = map.add_file("main.ptk", "Foo = (bar)");

        assert_eq!(map.content(id), "Foo = (bar)");
        assert_eq!(map.kind(id), SourceKind::File("main.ptk"));
    }

    #[test]
    fn multiple_sources() {
        let mut map = SourceMap::new();
        let a = map.add_file("a.ptk", "content a");
        let b = map.add_file("b.ptk", "content b");
        let c = map.add_one_liner("inline");
        let d = map.add_stdin("piped");

        assert_eq!(map.len(), 4);
        assert_eq!(map.content(a), "content a");
        assert_eq!(map.content(b), "content b");
        assert_eq!(map.content(c), "inline");
        assert_eq!(map.content(d), "piped");

        assert_eq!(map.kind(a), SourceKind::File("a.ptk"));
        assert_eq!(map.kind(b), SourceKind::File("b.ptk"));
        assert_eq!(map.kind(c), SourceKind::OneLiner);
        assert_eq!(map.kind(d), SourceKind::Stdin);
    }

    #[test]
    fn iteration() {
        let mut map = SourceMap::new();
        map.add_file("a.ptk", "aaa");
        map.add_one_liner("bbb");

        let items: Vec<_> = map.iter().collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, SourceId(0));
        assert_eq!(items[0].kind, SourceKind::File("a.ptk"));
        assert_eq!(items[0].content, "aaa");
        assert_eq!(items[1].id, SourceId(1));
        assert_eq!(items[1].kind, SourceKind::OneLiner);
        assert_eq!(items[1].content, "bbb");
    }

    #[test]
    fn get_source() {
        let mut map = SourceMap::new();
        let id = map.add_file("test.ptk", "hello");

        let source = map.get(id);
        assert_eq!(source.id, id);
        assert_eq!(source.kind, SourceKind::File("test.ptk"));
        assert_eq!(source.content, "hello");
        assert_eq!(source.as_str(), "hello");
    }

    #[test]
    fn display_name() {
        assert_eq!(SourceKind::OneLiner.display_name(), "<query>");
        assert_eq!(SourceKind::Stdin.display_name(), "<stdin>");
        assert_eq!(SourceKind::File("foo.ptk").display_name(), "foo.ptk");
    }

    #[test]
    #[should_panic(expected = "invalid SourceId")]
    fn invalid_id_panics() {
        let map = SourceMap::new();
        let _ = map.content(SourceId(999));
    }

    #[test]
    fn shared_buffer_lifetime() {
        let mut map = SourceMap::new();
        map.add_file("a", "first");
        map.add_file("b", "second");

        // Both slices have the same lifetime as &map
        let a_content = map.content(SourceId(0));
        let b_content = map.content(SourceId(1));

        // Can use both simultaneously
        assert_eq!(format!("{} {}", a_content, b_content), "first second");
    }

    #[test]
    fn multiple_stdin_sources() {
        let mut map = SourceMap::new();
        let a = map.add_stdin("first stdin");
        let b = map.add_stdin("second stdin");

        assert_eq!(map.content(a), "first stdin");
        assert_eq!(map.content(b), "second stdin");
        assert_eq!(map.kind(a), SourceKind::Stdin);
        assert_eq!(map.kind(b), SourceKind::Stdin);
    }
}
