use tree_sitter::Node;

use crate::alloc::{Buffer, Meter};
use crate::{LimitExceeded, TapeMark};

/// One logical output or inspection event.
///
/// The tape stores events compactly; this enum is their decoded form, used by
/// tracers and conformance checks. `PartialEq` compares `Node`s by
/// Tree-sitter identity, so two executors over one tree compare node for node.
#[derive(Clone, Copy, Debug, PartialEq)]
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
    /// Select the record field that receives the next value.
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
    /// Contribute a node-pattern match to the innermost scalar frame.
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

impl JournalEvent<'_> {
    /// Inspection events carry query provenance, not result data.
    pub fn is_inspection(&self) -> bool {
        matches!(self, Self::SpanStart { .. } | Self::SpanEnd(_))
    }
}

/// One encoded event: an opcode and a 32-bit operand.
///
/// Node payloads live in a separate buffer in event order, so an operation
/// that carries a node consumes the next node entry when decoding.
#[derive(Clone, Copy)]
#[repr(C)]
struct Op {
    code: u32,
    arg: u32,
}

const _: () = assert!(size_of::<Op>() == 8);

mod code {
    pub const NODE: u32 = 0;
    pub const LIST_OPEN: u32 = 1;
    pub const ARRAY_PUSH: u32 = 2;
    pub const LIST_CLOSE: u32 = 3;
    pub const RECORD_OPEN: u32 = 4;
    pub const RECORD_SET: u32 = 5;
    pub const RECORD_CLOSE: u32 = 6;
    pub const VARIANT_OPEN: u32 = 7;
    pub const VARIANT_CLOSE: u32 = 8;
    pub const ABSENT: u32 = 9;
    pub const SCALAR_OPEN: u32 = 10;
    pub const SCALAR_MARK: u32 = 11;
    pub const TEXT_CLOSE: u32 = 12;
    pub const BOOL_CLOSE: u32 = 13;
    pub const NODE_TEXT: u32 = 14;
    pub const NODE_BOOL: u32 = 15;
    pub const BOOL_VALUE: u32 = 16;
    pub const SPAN_START: u32 = 17;
    pub const SPAN_START_AT: u32 = 18;
    pub const SPAN_END: u32 = 19;
}

/// Append-only, reversible record of one candidate's output.
///
/// Appends are amortized O(1); a checkpoint is two lengths and rollback
/// restores them. Matching only appends and rolls back: it never builds result
/// values or reads earlier events.
pub struct Tape<'t> {
    ops: Buffer<Op>,
    nodes: Buffer<Node<'t>>,
    inspection: Buffer<Inspection<'t>>,
}

#[derive(Clone, Copy)]
struct Inspection<'t> {
    position: usize,
    event: JournalEvent<'t>,
}

impl<'t> Tape<'t> {
    pub fn new() -> Self {
        Self {
            ops: Buffer::new(),
            nodes: Buffer::new(),
            inspection: Buffer::new(),
        }
    }

    /// Number of recorded events.
    #[inline]
    pub fn len(&self) -> usize {
        self.ops.len() + self.inspection.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty() && self.inspection.is_empty()
    }

    #[inline]
    pub fn mark(&self) -> TapeMark {
        TapeMark {
            ops: self.ops.len(),
            nodes: self.nodes.len(),
            inspection: self.inspection.len(),
        }
    }

    /// Roll back every event recorded after `mark`.
    #[inline]
    pub fn rollback(&mut self, mark: TapeMark) {
        self.ops.truncate(mark.ops);
        self.nodes.truncate(mark.nodes);
        self.inspection.truncate(mark.inspection);
    }

    /// Record one event.
    pub fn push(
        &mut self,
        meter: &mut Meter<'_>,
        event: JournalEvent<'t>,
    ) -> Result<(), LimitExceeded> {
        if event.is_inspection() {
            return self.inspection.push(
                meter,
                Inspection {
                    position: self.ops.len(),
                    event,
                },
            );
        }

        let (code, arg, node) = encode(event);
        self.ops.reserve(meter, 1)?;
        if let Some(node) = node {
            self.nodes.push(meter, node)?;
        }
        self.ops.push(meter, Op { code, arg })
    }

    /// Decode the recorded events in order.
    pub fn events(&self) -> Events<'_, 't> {
        Events {
            ops: self.ops.as_slice().iter(),
            nodes: self.nodes.as_slice().iter(),
            inspection: self.inspection.as_slice().iter().peekable(),
            position: 0,
        }
    }

    /// Bytes of capacity the tape holds charged.
    pub fn capacity_bytes(&self) -> u64 {
        self.ops.capacity_bytes() + self.nodes.capacity_bytes() + self.inspection.capacity_bytes()
    }
}

impl Default for Tape<'_> {
    fn default() -> Self {
        Self::new()
    }
}

fn encode(event: JournalEvent<'_>) -> (u32, u32, Option<Node<'_>>) {
    match event {
        JournalEvent::Node(node) => (code::NODE, 0, Some(node)),
        JournalEvent::ListOpen => (code::LIST_OPEN, 0, None),
        JournalEvent::ArrayPush => (code::ARRAY_PUSH, 0, None),
        JournalEvent::ListClose => (code::LIST_CLOSE, 0, None),
        JournalEvent::RecordOpen => (code::RECORD_OPEN, 0, None),
        JournalEvent::RecordSet(member) => (code::RECORD_SET, u32::from(member), None),
        JournalEvent::RecordClose => (code::RECORD_CLOSE, 0, None),
        JournalEvent::VariantOpen(member) => (code::VARIANT_OPEN, u32::from(member), None),
        JournalEvent::VariantClose => (code::VARIANT_CLOSE, 0, None),
        JournalEvent::Absent => (code::ABSENT, 0, None),
        JournalEvent::ScalarOpen => (code::SCALAR_OPEN, 0, None),
        JournalEvent::ScalarMark(node) => (code::SCALAR_MARK, 0, Some(node)),
        JournalEvent::TextClose => (code::TEXT_CLOSE, 0, None),
        JournalEvent::BoolClose(value) => (code::BOOL_CLOSE, u32::from(value), None),
        JournalEvent::NodeText(node) => (code::NODE_TEXT, 0, Some(node)),
        JournalEvent::NodeBool(node) => (code::NODE_BOOL, 0, Some(node)),
        JournalEvent::BoolValue(value) => (code::BOOL_VALUE, u32::from(value), None),
        JournalEvent::SpanStart { id, node: None } => (code::SPAN_START, u32::from(id), None),
        JournalEvent::SpanStart { id, node } => (code::SPAN_START_AT, u32::from(id), node),
        JournalEvent::SpanEnd(id) => (code::SPAN_END, u32::from(id), None),
    }
}

/// Decoding iterator over a tape.
pub struct Events<'a, 't> {
    ops: std::slice::Iter<'a, Op>,
    nodes: std::slice::Iter<'a, Node<'t>>,
    inspection: std::iter::Peekable<std::slice::Iter<'a, Inspection<'t>>>,
    position: usize,
}

impl<'t> Iterator for Events<'_, 't> {
    type Item = JournalEvent<'t>;

    fn next(&mut self) -> Option<JournalEvent<'t>> {
        if self
            .inspection
            .peek()
            .is_some_and(|event| event.position == self.position)
        {
            return Some(
                self.inspection
                    .next()
                    .expect("peeked inspection event exists")
                    .event,
            );
        }

        let op = *self.ops.next()?;
        self.position += 1;
        let mut node = || {
            *self
                .nodes
                .next()
                .expect("every node-carrying tape operation has a node entry")
        };
        let member = || u16::try_from(op.arg).expect("tape member operands fit u16");
        Some(match op.code {
            code::NODE => JournalEvent::Node(node()),
            code::LIST_OPEN => JournalEvent::ListOpen,
            code::ARRAY_PUSH => JournalEvent::ArrayPush,
            code::LIST_CLOSE => JournalEvent::ListClose,
            code::RECORD_OPEN => JournalEvent::RecordOpen,
            code::RECORD_SET => JournalEvent::RecordSet(member()),
            code::RECORD_CLOSE => JournalEvent::RecordClose,
            code::VARIANT_OPEN => JournalEvent::VariantOpen(member()),
            code::VARIANT_CLOSE => JournalEvent::VariantClose,
            code::ABSENT => JournalEvent::Absent,
            code::SCALAR_OPEN => JournalEvent::ScalarOpen,
            code::SCALAR_MARK => JournalEvent::ScalarMark(node()),
            code::TEXT_CLOSE => JournalEvent::TextClose,
            code::BOOL_CLOSE => JournalEvent::BoolClose(op.arg != 0),
            code::NODE_TEXT => JournalEvent::NodeText(node()),
            code::NODE_BOOL => JournalEvent::NodeBool(node()),
            code::BOOL_VALUE => JournalEvent::BoolValue(op.arg != 0),
            code::SPAN_START => JournalEvent::SpanStart {
                id: member(),
                node: None,
            },
            code::SPAN_START_AT => JournalEvent::SpanStart {
                id: member(),
                node: Some(node()),
            },
            code::SPAN_END => JournalEvent::SpanEnd(member()),
            other => unreachable!("tape holds only encoded events, found opcode {other}"),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.ops.len() + self.inspection.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for Events<'_, '_> {}

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

/// Read a node's source text for a predicate, charging fuel for its length
/// at the same rate as other byte-proportional work.
pub fn predicate_text<'s>(
    meter: &mut Meter<'_>,
    source: &'s str,
    node: &Node<'_>,
) -> Result<&'s str, LimitExceeded> {
    let bytes = node.end_byte().saturating_sub(node.start_byte()) as u64;
    meter.charge(1)?;
    meter.charge_bytes(bytes)?;
    Ok(node_text(source, node))
}
