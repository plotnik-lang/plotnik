//! Source storage for query compilation.

/// Lightweight handle to a source in a compilation session.
///
/// `Ord` follows insertion order (file index); diagnostics sort by it to group
/// by file before falling back to in-file offset.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub struct SourceId(pub(crate) u32);

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SourceKind {
    /// An inline query passed directly (e.g., CLI `-q` argument).
    Inline,
    /// Input read from stdin.
    Stdin,
    /// A file with its path.
    File(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourcePath<'a>(&'a str);

impl<'a> SourcePath<'a> {
    pub fn new(path: &'a str) -> Self {
        Self(path)
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }
}

impl SourceKind {
    pub fn display_name(&self) -> &str {
        match self {
            SourceKind::Inline => "<query>",
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

#[derive(Clone, Debug)]
struct SourceEntry {
    kind: SourceKind,
    content: String,
}

#[derive(Clone, Debug, Default)]
pub struct SourceMap {
    entries: Vec<SourceEntry>,
}

impl SourceMap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an inline source (CLI `-q` argument, REPL, tests).
    pub fn add_inline(&mut self, content: &str) -> SourceId {
        self.push_entry(SourceKind::Inline, content)
    }

    pub fn add_stdin(&mut self, content: &str) -> SourceId {
        self.push_entry(SourceKind::Stdin, content)
    }

    pub fn add_file(&mut self, path: SourcePath<'_>, content: &str) -> SourceId {
        self.push_entry(SourceKind::File(path.as_str().to_owned()), content)
    }

    /// Convenience for single-source use cases (CLI, REPL, tests).
    pub fn from_inline(content: &str) -> Self {
        let mut map = Self::new();
        map.add_inline(content);
        map
    }

    pub fn content(&self, id: SourceId) -> &str {
        self.entries
            .get(id.0 as usize)
            .map(|e| e.content.as_str())
            .expect("invalid SourceId")
    }

    pub fn kind(&self, id: SourceId) -> &SourceKind {
        self.entries
            .get(id.0 as usize)
            .map(|e| &e.kind)
            .expect("invalid SourceId")
    }

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

    pub fn source(&self, id: SourceId) -> Source<'_> {
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
