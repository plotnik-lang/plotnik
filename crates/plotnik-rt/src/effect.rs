//! Runtime effects for VM execution.
//!
//! Runtime effects carry actual node references, unlike bytecode `Effect`
//! which only stores kind + payload.

use tree_sitter::Node;

/// `PartialEq` compares `Node`s by tree-sitter identity (same node in the same
/// tree), which is exactly what conformance harnesses need: two executors run
/// over one parse tree must produce identical streams, node-for-node.
#[derive(Debug, PartialEq)]
pub enum RuntimeEffect<'t> {
    /// Capture a node reference.
    Node(Node<'t>),
    /// Begin array scope.
    ArrayOpen,
    /// Push current value to array.
    Push,
    /// End array scope.
    ArrayClose,
    /// Begin struct scope.
    StructOpen,
    /// Set field at member index.
    Set(u16),
    /// End struct scope.
    StructClose,
    /// Begin a variant case at its member index.
    VariantOpen(u16),
    /// End a variant case.
    VariantClose,
    /// Null placeholder (for optional/alternation).
    Null,
    /// Begin one value-local scalar provenance frame.
    ScalarOpen,
    /// Contribute an explicit node-pattern match to every open scalar frame.
    ScalarMark(Node<'t>),
    /// Close a scalar frame and materialize its source text (or null when unmarked).
    StrClose,
    /// Close a scalar frame and materialize the supplied boolean.
    BoolClose(bool),
    /// Materialize the matched node's source text without a scalar frame.
    NodeStr(Node<'t>),
    /// Materialize presence for the matched node without a scalar frame.
    NodeBool(Node<'t>),
    /// Materialize a boolean with no source provenance.
    BoolValue(bool),
    /// Open an inspection span. `node` is present only for cursor-snapshot starts.
    SpanStart { id: u16, node: Option<Node<'t>> },
    /// Close an inspection span.
    SpanEnd(u16),
}

/// Effect log with truncation support for backtracking.
#[derive(Debug)]
pub struct EffectLog<'t>(Vec<RuntimeEffect<'t>>);

impl<'t> EffectLog<'t> {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    #[inline]
    pub fn push(&mut self, effect: RuntimeEffect<'t>) {
        self.0.push(effect);
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
        (self.0.len() * std::mem::size_of::<RuntimeEffect<'t>>()) as u64
    }

    /// Check if empty.
    #[inline]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Truncate to a saved watermark, rolling back effects on backtrack.
    #[inline]
    pub fn truncate(&mut self, watermark: usize) {
        self.0.truncate(watermark);
    }

    pub fn as_slice(&self) -> &[RuntimeEffect<'t>] {
        &self.0
    }
}

impl Default for EffectLog<'_> {
    fn default() -> Self {
        Self::new()
    }
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
