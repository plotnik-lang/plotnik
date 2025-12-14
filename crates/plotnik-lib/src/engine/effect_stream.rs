//! Effect stream recorded during query execution.

use crate::ir::EffectOp;
use serde::Serialize;
use serde::ser::SerializeStruct;
use tree_sitter::Node;

/// A captured AST node with a reference to the source.
#[derive(Debug, Clone, Copy)]
pub struct CapturedNode<'tree> {
    node: Node<'tree>,
    source: &'tree str,
}

impl<'tree> CapturedNode<'tree> {
    /// Create from a tree-sitter node and source text.
    pub fn new(node: Node<'tree>, source: &'tree str) -> Self {
        Self { node, source }
    }

    /// Returns the underlying tree-sitter node.
    pub fn node(&self) -> Node<'tree> {
        self.node
    }

    /// Returns the source text of the node.
    pub fn text(&self) -> &'tree str {
        self.node
            .utf8_text(self.source.as_bytes())
            .unwrap_or("<invalid utf8>")
    }

    pub fn start_byte(&self) -> usize {
        self.node.start_byte()
    }

    pub fn end_byte(&self) -> usize {
        self.node.end_byte()
    }

    pub fn start_point(&self) -> (usize, usize) {
        let p = self.node.start_position();
        (p.row, p.column)
    }

    pub fn end_point(&self) -> (usize, usize) {
        let p = self.node.end_position();
        (p.row, p.column)
    }

    pub fn kind(&self) -> &'tree str {
        self.node.kind()
    }
}

impl PartialEq for CapturedNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        // Compare by node identity (same position in same tree)
        self.node.id() == other.node.id()
            && self.start_byte() == other.start_byte()
            && self.end_byte() == other.end_byte()
    }
}

impl Eq for CapturedNode<'_> {}

impl Serialize for CapturedNode<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("CapturedNode", 3)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("text", self.text())?;
        state.serialize_field("range", &[self.start_byte(), self.end_byte()])?;
        state.end()
    }
}

/// Wrapper for verbose serialization of a captured node.
/// Includes full positional information (bytes + line/column).
pub struct VerboseNode<'a, 'tree>(pub &'a CapturedNode<'tree>);

impl Serialize for VerboseNode<'_, '_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let node = self.0;
        let mut state = serializer.serialize_struct("CapturedNode", 6)?;
        state.serialize_field("kind", node.kind())?;
        state.serialize_field("text", node.text())?;
        state.serialize_field("start_byte", &node.start_byte())?;
        state.serialize_field("end_byte", &node.end_byte())?;
        state.serialize_field("start_point", &node.start_point())?;
        state.serialize_field("end_point", &node.end_point())?;
        state.end()
    }
}

/// A log of effects to be replayed by the materializer.
/// See ADR-0006 for details.
#[derive(Debug, Clone, Default)]
pub struct EffectStream<'tree> {
    /// The sequence of operations to perform.
    ops: Vec<EffectOp>,
    /// The sequence of nodes captured, one for each `CaptureNode` op.
    nodes: Vec<CapturedNode<'tree>>,
}

impl<'tree> EffectStream<'tree> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends an effect operation to the stream.
    pub fn push_op(&mut self, op: EffectOp) {
        self.ops.push(op);
    }

    /// Appends a captured node to the stream.
    pub fn push_node(&mut self, node: Node<'tree>, source: &'tree str) {
        self.nodes.push(CapturedNode::new(node, source));
    }

    /// Appends a captured node directly.
    pub fn push_captured_node(&mut self, node: CapturedNode<'tree>) {
        self.nodes.push(node);
    }

    /// Returns the operations.
    pub fn ops(&self) -> &[EffectOp] {
        &self.ops
    }

    /// Returns the captured nodes.
    pub fn nodes(&self) -> &[CapturedNode<'tree>] {
        &self.nodes
    }

    /// Truncate streams to watermarks (for backtracking).
    pub fn truncate(&mut self, ops_len: usize, nodes_len: usize) {
        self.ops.truncate(ops_len);
        self.nodes.truncate(nodes_len);
    }
}
