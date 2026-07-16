//! Rollbackable journal entries recorded during matching.
//!
//! Journal events carry actual node references, unlike bytecode `Effect`
//! which only stores kind + payload.

use std::ops::Index;

use tree_sitter::Node;

/// `PartialEq` compares `Node`s by tree-sitter identity (same node in the same
/// tree), which is exactly what conformance harnesses need: two executors run
/// over one parse tree must produce identical streams, node-for-node.
#[derive(Debug, PartialEq)]
pub enum JournalEvent<'t> {
    /// Capture a node reference.
    Node(Node<'t>),
    /// Begin a list value.
    ListOpen,
    /// Append the current value to the open list.
    ArrayPush,
    /// End a list value.
    ListClose,
    /// Begin a record value.
    RecordOpen,
    /// Set the record field at the given member index.
    RecordSet(u16),
    /// End a record value.
    RecordClose,
    /// Begin a variant case at its member index.
    VariantOpen(u16),
    /// End a variant case.
    VariantClose,
    /// Produce an absent value for field completion or an unmatched option.
    Absent,
    /// Begin one value-local scalar provenance frame.
    ScalarOpen,
    /// Contribute an explicit node-pattern match to every open scalar frame.
    ScalarMark(Node<'t>),
    /// Close a scalar frame and materialize its source text (or null when unmarked).
    TextClose,
    /// Close a scalar frame and materialize the supplied boolean.
    BoolClose(bool),
    /// Materialize the matched node's source text without a scalar frame.
    NodeText(Node<'t>),
    /// Materialize presence for the matched node without a scalar frame.
    NodeBool(Node<'t>),
    /// Materialize a boolean with no source provenance.
    BoolValue(bool),
    /// Open an inspection span. `node` is present only for cursor-snapshot starts.
    SpanStart { id: u16, node: Option<Node<'t>> },
    /// Close an inspection span.
    SpanEnd(u16),
}

/// Match journal with truncation support for backtracking.
#[derive(Debug)]
pub struct MatchJournal<'t>(Vec<JournalEvent<'t>>);

impl<'t> MatchJournal<'t> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[inline]
    pub fn push(&mut self, event: JournalEvent<'t>) {
        self.0.push(event);
    }

    /// Get current length (used as watermark for backtracking).
    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Live heap bytes: occupied element count × element size. Spare `Vec`
    /// capacity is not counted — this measures live data, not the allocation.
    #[inline]
    pub fn byte_footprint(&self) -> u64 {
        (self.0.len() * std::mem::size_of::<JournalEvent<'t>>()) as u64
    }

    /// Check if empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Truncate to a saved watermark, rolling back events on backtrack.
    #[inline]
    pub fn truncate(&mut self, watermark: usize) {
        self.0.truncate(watermark);
    }

    pub fn as_slice(&self) -> &[JournalEvent<'t>] {
        &self.0
    }

    /// Logical result-construction view of the committed journal.
    ///
    /// Generated matchers record no inspection events, so their view borrows the
    /// journal directly. Inspection-enabled VM journals build an index that omits
    /// `SpanStart`/`SpanEnd` without copying output events.
    pub fn output_events(&self) -> OutputEvents<'_, 't> {
        OutputEvents::new(&self.0)
    }
}

impl Default for MatchJournal<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// Result-construction events from a committed [`MatchJournal`].
#[derive(Debug)]
pub struct OutputEvents<'a, 't> {
    journal: &'a [JournalEvent<'t>],
    positions: Option<Vec<usize>>,
}

impl<'a, 't> OutputEvents<'a, 't> {
    fn new(journal: &'a [JournalEvent<'t>]) -> Self {
        let Some(first_inspection) = journal.iter().position(is_inspection_event) else {
            return Self {
                journal,
                positions: None,
            };
        };
        let positions = (0..first_inspection)
            .chain(
                (first_inspection..journal.len())
                    .filter(|&index| !is_inspection_event(&journal[index])),
            )
            .collect();
        Self {
            journal,
            positions: Some(positions),
        }
    }

    pub fn len(&self) -> usize {
        self.positions.as_ref().map_or(self.journal.len(), Vec::len)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, index: usize) -> Option<&'a JournalEvent<'t>> {
        let journal_index = self
            .positions
            .as_ref()
            .map_or(Some(index), |positions| positions.get(index).copied())?;
        self.journal.get(journal_index)
    }

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = &'a JournalEvent<'t>> + ExactSizeIterator + '_ {
        (0..self.len()).map(|index| {
            self.get(index)
                .expect("output-event index comes from the view's own length")
        })
    }
}

impl<'t> Index<usize> for OutputEvents<'_, 't> {
    type Output = JournalEvent<'t>;

    fn index(&self, index: usize) -> &Self::Output {
        self.get(index)
            .expect("output-event index must be within the view")
    }
}

fn is_inspection_event(event: &JournalEvent<'_>) -> bool {
    matches!(
        event,
        JournalEvent::SpanStart { .. } | JournalEvent::SpanEnd(_)
    )
}

/// The text a node spans in `source` — what string and regex predicates
/// compare against. Shared by the VM and generated matchers so both slice
/// identically.
///
/// tree-sitter byte offsets fall on character boundaries of the valid-UTF-8
/// source, so the fallible `get` turns a violated expectation into a named
/// panic instead of a raw slice abort.
pub fn node_text<'s>(source: &'s str, node: &Node<'_>) -> &'s str {
    source_text(source, node.start_byte()..node.end_byte())
}

/// Slice one validated scalar provenance range from the source.
pub fn source_text(source: &str, range: std::ops::Range<usize>) -> &str {
    source
        .get(range)
        .expect("node span must lie within source on UTF-8 boundaries")
}
