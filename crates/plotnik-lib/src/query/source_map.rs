//! Source storage for query compilation.
//!
//! Stores sources as owned strings, providing a simple interface for
//! multi-source compilation sessions.

/// Lightweight handle to a source in a compilation session.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct SourceId(u32);

/// Describes the origin of a source.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// A one-liner query passed directly (e.g., CLI `-q` argument).
    OneLiner,
    /// Input read from stdin.
    Stdin,
    /// A file with its path.
    File(String),
}

impl SourceKind {
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
#[derive(Clone, Debug)]
pub struct Source<'q> {
    pub id: SourceId,
    pub kind: &'q SourceKind,
    pub content: &'q str,
}

impl<'q> Source<'q> {
    /// Returns the content string.
    pub fn as_str(&self) -> &'q str {
        self.content
    }
}

/// Metadata for a source.
#[derive(Clone, Debug)]
struct SourceEntry {
    kind: SourceKind,
    content: String,
}

/// Registry of all sources.
#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    entries: Vec<SourceEntry>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a one-liner source (CLI `-q` argument, REPL, tests).
    pub fn add_one_liner(&mut self, content: &str) -> SourceId {
        self.push_entry(SourceKind::OneLiner, content)
    }

    /// Add a source read from stdin.
    pub fn add_stdin(&mut self, content: &str) -> SourceId {
        self.push_entry(SourceKind::Stdin, content)
    }

    /// Add a file source with its path.
    pub fn add_file(&mut self, path: &str, content: &str) -> SourceId {
        self.push_entry(SourceKind::File(path.to_owned()), content)
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
            .map(|e| e.content.as_str())
            .expect("invalid SourceId")
    }

    /// Get the kind of a source by ID.
    pub fn kind(&self, id: SourceId) -> &SourceKind {
        self.entries
            .get(id.0 as usize)
            .map(|e| &e.kind)
            .expect("invalid SourceId")
    }

    /// Get the file path if this source is a file, None otherwise.
    pub fn path(&self, id: SourceId) -> Option<&str> {
        let entry = self.entries.get(id.0 as usize).expect("invalid SourceId");
        match &entry.kind {
            SourceKind::File(path) => Some(path),
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
            kind: &entry.kind,
            content: &entry.content,
        }
    }

    /// Iterate over all sources as `Source` views.
    pub fn iter(&self) -> impl Iterator<Item = Source<'_>> {
        self.entries.iter().enumerate().map(|(idx, entry)| Source {
            id: SourceId(idx as u32),
            kind: &entry.kind,
            content: &entry.content,
        })
    }

    fn push_entry(&mut self, kind: SourceKind, content: &str) -> SourceId {
        let id = SourceId(self.entries.len() as u32);
        self.entries.push(SourceEntry {
            kind,
            content: content.to_owned(),
        });
        id
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
        assert_eq!(map.kind(id), &SourceKind::OneLiner);
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn stdin_source() {
        let mut map = SourceMap::new();
        let id = map.add_stdin("from stdin");

        assert_eq!(map.content(id), "from stdin");
        assert_eq!(map.kind(id), &SourceKind::Stdin);
    }

    #[test]
    fn file_source() {
        let mut map = SourceMap::new();
        let id = map.add_file("main.ptk", "Foo = (bar)");

        assert_eq!(map.content(id), "Foo = (bar)");
        assert_eq!(map.kind(id), &SourceKind::File("main.ptk".to_owned()));
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

        assert_eq!(map.kind(a), &SourceKind::File("a.ptk".to_owned()));
        assert_eq!(map.kind(b), &SourceKind::File("b.ptk".to_owned()));
        assert_eq!(map.kind(c), &SourceKind::OneLiner);
        assert_eq!(map.kind(d), &SourceKind::Stdin);
    }

    #[test]
    fn iteration() {
        let mut map = SourceMap::new();
        map.add_file("a.ptk", "aaa");
        map.add_one_liner("bbb");

        let items: Vec<_> = map.iter().collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, SourceId(0));
        assert_eq!(items[0].kind, &SourceKind::File("a.ptk".to_owned()));
        assert_eq!(items[0].content, "aaa");
        assert_eq!(items[1].id, SourceId(1));
        assert_eq!(items[1].kind, &SourceKind::OneLiner);
        assert_eq!(items[1].content, "bbb");
    }

    #[test]
    fn get_source() {
        let mut map = SourceMap::new();
        let id = map.add_file("test.ptk", "hello");

        let source = map.get(id);
        assert_eq!(source.id, id);
        assert_eq!(source.kind, &SourceKind::File("test.ptk".to_owned()));
        assert_eq!(source.content, "hello");
        assert_eq!(source.as_str(), "hello");
    }

    #[test]
    fn display_name() {
        assert_eq!(SourceKind::OneLiner.display_name(), "<query>");
        assert_eq!(SourceKind::Stdin.display_name(), "<stdin>");
        assert_eq!(
            SourceKind::File("foo.ptk".to_owned()).display_name(),
            "foo.ptk"
        );
    }

    #[test]
    #[should_panic(expected = "invalid SourceId")]
    fn invalid_id_panics() {
        let map = SourceMap::new();
        let _ = map.content(SourceId(999));
    }

    #[test]
    fn multiple_stdin_sources() {
        let mut map = SourceMap::new();
        let a = map.add_stdin("first stdin");
        let b = map.add_stdin("second stdin");

        assert_eq!(map.content(a), "first stdin");
        assert_eq!(map.content(b), "second stdin");
        assert_eq!(map.kind(a), &SourceKind::Stdin);
        assert_eq!(map.kind(b), &SourceKind::Stdin);
    }
}
