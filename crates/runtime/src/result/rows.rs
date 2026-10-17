use tree_sitter::Node;

use super::schema::Schema;
use crate::LimitExceeded;
use crate::alloc::{Buffer, Meter};
use crate::tape::JournalEvent;

/// An edge slot no `RecordSet` filled.
pub(crate) const MISSING: u32 = u32::MAX;

const PENDING: u32 = u32::MAX - 1;

/// A record that saw no `RecordSet`, so the events do not name its type.
pub(crate) const UNTYPED: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum Kind {
    /// `a`: index into [`Rows::nodes`].
    Node,
    /// `a..b`: byte range of the source.
    Text,
    /// `a`: 0 or 1.
    Bool,
    Absent,
    /// `a`: first edge, `b`: element count.
    List,
    /// `a`: first edge, `b`: record type or [`UNTYPED`], `c`: field count.
    Record,
    /// `a`: first edge, `b`: variant type, `c`: case index.
    Variant,
}

/// One normalized value. Rows are appended in postorder, so every child
/// precedes its parent.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Row {
    pub kind: Kind,
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl Row {
    fn new(kind: Kind, a: u32, b: u32, c: u32) -> Self {
        Self { kind, a, b, c }
    }
}

/// The accepted output of one match as flat, indexed rows.
///
/// Records and variant cases keep one edge per field in canonical field
/// order; unset fields hold [`MISSING`]. Lists keep their elements in order.
pub struct Rows<'t> {
    pub(crate) rows: Buffer<Row>,
    pub(crate) edges: Buffer<u32>,
    pub(crate) nodes: Buffer<Node<'t>>,
}

impl<'t> Rows<'t> {
    pub fn new() -> Self {
        Self {
            rows: Buffer::new(),
            edges: Buffer::new(),
            nodes: Buffer::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.rows.clear();
        self.edges.clear();
        self.nodes.clear();
    }

    pub(crate) fn row(&self, id: u32) -> Row {
        self.rows[id as usize]
    }

    pub(crate) fn edges(&self, start: u32, len: u32) -> &[u32] {
        let start = start as usize;
        &self.edges.as_slice()[start..start + len as usize]
    }

    pub(crate) fn node(&self, index: u32) -> Node<'t> {
        self.nodes[index as usize]
    }

    fn push(&mut self, meter: &mut Meter<'_>, row: Row) -> Result<u32, LimitExceeded> {
        let id = index(self.rows.len())?;
        self.rows.push(meter, row)?;
        Ok(id)
    }

    fn edge_start(&self) -> Result<u32, LimitExceeded> {
        index(self.edges.len())
    }
}

impl Default for Rows<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
enum FrameKind {
    List,
    Record,
    Variant,
    Scalar,
}

/// One open value while replaying the tape.
///
/// Lists collect child ids from `base` in the child stack. Records and
/// variants keep one cell per field from `base` in the cell stack; a record
/// allocates its cells at its first `RecordSet`, before that field's value
/// opens. Scalars keep their marked byte range in `base..end`.
#[derive(Clone, Copy)]
struct Frame {
    kind: FrameKind,
    owner: u32,
    case: u32,
    base: u32,
    end: u32,
    selected: u32,
}

/// Reusable scratch for [`Normalizer::run`].
pub struct Normalizer {
    frames: Buffer<Frame>,
    cells: Buffer<u32>,
    children: Buffer<u32>,
}

impl Normalizer {
    pub fn new() -> Self {
        Self {
            frames: Buffer::new(),
            cells: Buffer::new(),
            children: Buffer::new(),
        }
    }

    /// Replay accepted output events into `rows`, returning the root row.
    ///
    /// The replay is iterative: native stack use does not depend on result
    /// depth. It checks the event structure against `schema` and panics on
    /// a violation, because only a verified matcher produces these events.
    pub fn run<'t>(
        &mut self,
        meter: &mut Meter<'_>,
        schema: &Schema<'_>,
        events: impl Iterator<Item = JournalEvent<'t>>,
        rows: &mut Rows<'t>,
    ) -> Result<u32, LimitExceeded> {
        rows.clear();
        self.frames.clear();
        self.cells.clear();
        self.children.clear();

        let mut pending: Option<u32> = None;
        for event in events {
            meter.charge(1)?;
            match event {
                JournalEvent::Node(node) => {
                    let index = index(rows.nodes.len())?;
                    rows.nodes.push(meter, node)?;
                    let row = rows.push(meter, Row::new(Kind::Node, index, 0, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::Absent => {
                    let row = rows.push(meter, Row::new(Kind::Absent, 0, 0, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::NodeText(node) => {
                    let (start, end) = byte_range(&node);
                    let row = rows.push(meter, Row::new(Kind::Text, start, end, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::NodeBool(_) => {
                    let row = rows.push(meter, Row::new(Kind::Bool, 1, 0, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::BoolValue(value) => {
                    let row = rows.push(meter, Row::new(Kind::Bool, u32::from(value), 0, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::ScalarOpen => {
                    self.open(meter, FrameKind::Scalar, MISSING, 0, MISSING)?;
                }
                JournalEvent::ScalarMark(node) => {
                    let frame = self
                        .frames
                        .last_mut()
                        .expect("a scalar mark has an open scalar frame");
                    assert!(
                        matches!(frame.kind, FrameKind::Scalar),
                        "a scalar mark arrived inside a structural value"
                    );
                    let (start, end) = byte_range(&node);
                    widen(frame, start, end);
                }
                JournalEvent::TextClose => {
                    let (start, end) = self.close_scalar();
                    let row = if start == MISSING {
                        Row::new(Kind::Absent, 0, 0, 0)
                    } else {
                        Row::new(Kind::Text, start, end, 0)
                    };
                    let row = rows.push(meter, row)?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::BoolClose(value) => {
                    self.close_scalar();
                    let row = rows.push(meter, Row::new(Kind::Bool, u32::from(value), 0, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::SpanStart { .. } | JournalEvent::SpanEnd(_) => {}
                JournalEvent::ListOpen => {
                    assert!(pending.is_none(), "a list opened over an unconsumed value");
                    let base = index(self.children.len())?;
                    self.open(meter, FrameKind::List, MISSING, 0, base)?;
                }
                JournalEvent::ArrayPush => {
                    let frame = self.frames.last().expect("ArrayPush has an open list");
                    assert!(
                        matches!(frame.kind, FrameKind::List),
                        "ArrayPush arrived outside a list"
                    );
                    let value = pending.take().expect("ArrayPush follows a value");
                    self.children.push(meter, value)?;
                }
                JournalEvent::ListClose => {
                    let frame = self.close(FrameKind::List);
                    let start = rows.edge_start()?;
                    let base = frame.base as usize;
                    let len = index(self.children.len() - base)?;
                    rows.edges
                        .extend_from_slice(meter, &self.children.as_slice()[base..])?;
                    self.children.truncate(base);
                    let row = rows.push(meter, Row::new(Kind::List, start, len, 0))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::RecordOpen => {
                    assert!(
                        pending.is_none(),
                        "a record opened over an unconsumed value"
                    );
                    self.open(meter, FrameKind::Record, UNTYPED, 0, MISSING)?;
                }
                JournalEvent::RecordSet(member) => {
                    assert!(pending.is_none(), "RecordSet precedes its value");
                    self.set(meter, schema, u32::from(member))?;
                }
                JournalEvent::RecordClose => {
                    let frame = self.close(FrameKind::Record);
                    let start = rows.edge_start()?;
                    let len = self.flush_cells(meter, frame, rows)?;
                    let row = rows.push(meter, Row::new(Kind::Record, start, frame.owner, len))?;
                    self.produce(&mut pending, rows, row);
                }
                JournalEvent::VariantOpen(member) => {
                    assert!(
                        pending.is_none(),
                        "a variant opened over an unconsumed value"
                    );
                    let member = schema.member(u32::from(member));
                    let fields = schema.case_fields(member.owner, member.index).len();
                    let base = index(self.cells.len())?;
                    self.open(meter, FrameKind::Variant, member.owner, member.index, base)?;
                    self.missing_cells(meter, fields)?;
                }
                JournalEvent::VariantClose => {
                    let frame = self.close(FrameKind::Variant);
                    let start = if let Some(payload) = pending.take() {
                        let payload_ty = schema
                            .case(frame.owner, frame.case)
                            .payload()
                            .expect("tag-only case has no payload value");
                        let value = rows.row(payload);
                        assert!(
                            matches!(value.kind, Kind::Record),
                            "case payload is a record"
                        );
                        assert!(
                            value.b == payload_ty || value.b == UNTYPED,
                            "case payload has its declared public type"
                        );
                        let base = frame.base as usize;
                        meter.charge((self.cells.len() - base) as u64)?;
                        assert!(
                            self.cells.as_slice()[base..]
                                .iter()
                                .all(|&cell| cell == MISSING),
                            "case cannot combine a completed payload and direct fields"
                        );
                        self.cells.truncate(base);
                        if value.b == UNTYPED { MISSING } else { value.a }
                    } else {
                        let start = rows.edge_start()?;
                        self.flush_cells(meter, frame, rows)?;
                        start
                    };
                    let row = rows.push(
                        meter,
                        Row::new(Kind::Variant, start, frame.owner, frame.case),
                    )?;
                    self.produce(&mut pending, rows, row);
                }
            }
        }

        assert!(
            self.frames.is_empty(),
            "accepted output left {} value(s) open",
            self.frames.len()
        );
        Ok(pending.expect("accepted output produced a value"))
    }

    fn open(
        &mut self,
        meter: &mut Meter<'_>,
        kind: FrameKind,
        owner: u32,
        case: u32,
        base: u32,
    ) -> Result<(), LimitExceeded> {
        self.frames.push(
            meter,
            Frame {
                kind,
                owner,
                case,
                base,
                end: 0,
                selected: MISSING,
            },
        )
    }

    fn close(&mut self, kind: FrameKind) -> Frame {
        let frame = self.frames.pop().expect("a close event has an open value");
        assert!(
            std::mem::discriminant(&frame.kind) == std::mem::discriminant(&kind),
            "a close event does not match the innermost open value"
        );
        assert_eq!(frame.selected, MISSING, "a selected field has no value");
        frame
    }

    /// Close a scalar frame, folding its range into an enclosing scalar.
    fn close_scalar(&mut self) -> (u32, u32) {
        let frame = self.close(FrameKind::Scalar);
        let (start, end) = (frame.base, frame.end);
        if start != MISSING
            && let Some(parent) = self.frames.last_mut()
            && matches!(parent.kind, FrameKind::Scalar)
        {
            widen(parent, start, end);
        }
        (start, end)
    }

    fn missing_cells(&mut self, meter: &mut Meter<'_>, count: usize) -> Result<(), LimitExceeded> {
        self.cells.reserve(meter, count)?;
        for _ in 0..count {
            self.cells.push(meter, MISSING)?;
        }
        Ok(())
    }

    fn set(
        &mut self,
        meter: &mut Meter<'_>,
        schema: &Schema<'_>,
        member_id: u32,
    ) -> Result<(), LimitExceeded> {
        let member = schema.member(member_id);
        let base = self.cells.len();
        let frame = self
            .frames
            .last_mut()
            .expect("RecordSet has an open record or variant");
        assert_eq!(frame.selected, MISSING, "a selected field has no value");
        let fields = match frame.kind {
            FrameKind::Record => {
                if frame.owner == UNTYPED {
                    frame.owner = member.owner;
                    frame.base = index(base)?;
                    let fields = schema.fields(member.owner).len();
                    self.missing_cells(meter, fields)?;
                    fields
                } else {
                    assert_eq!(
                        member.owner, frame.owner,
                        "RecordSet member {member_id} belongs to another record"
                    );
                    schema.fields(frame.owner).len()
                }
            }
            FrameKind::Variant => {
                let payload = schema
                    .case(frame.owner, frame.case)
                    .payload()
                    .expect("RecordSet inside a tag-only variant case");
                assert_eq!(
                    member.owner, payload,
                    "RecordSet member {member_id} belongs to another case payload"
                );
                schema.fields(payload).len()
            }
            FrameKind::List | FrameKind::Scalar => {
                panic!("RecordSet arrived outside a record or variant")
            }
        };

        let frame = *self.frames.last().expect("frame checked above");
        assert!(
            (member.index as usize) < fields,
            "RecordSet member {member_id} is outside its record"
        );
        let cell = &mut self.cells[frame.base as usize + member.index as usize];
        assert_eq!(*cell, MISSING, "RecordSet member {member_id} was set twice");
        *cell = PENDING;
        self.frames
            .last_mut()
            .expect("frame checked above")
            .selected = member.index;
        Ok(())
    }

    fn produce(&mut self, pending: &mut Option<u32>, _rows: &Rows<'_>, row: u32) {
        if let Some(frame) = self.frames.last_mut()
            && frame.selected != MISSING
        {
            let cell = &mut self.cells[frame.base as usize + frame.selected as usize];
            assert_eq!(*cell, PENDING, "a selected field has a pending slot");
            *cell = row;
            frame.selected = MISSING;
            return;
        }

        produce(pending, row);
    }

    /// Move a closed record's or case's cells to permanent edges.
    fn flush_cells(
        &mut self,
        meter: &mut Meter<'_>,
        frame: Frame,
        rows: &mut Rows<'_>,
    ) -> Result<u32, LimitExceeded> {
        if frame.base == MISSING {
            return Ok(0);
        }
        let base = frame.base as usize;
        let len = index(self.cells.len() - base)?;
        rows.edges
            .extend_from_slice(meter, &self.cells.as_slice()[base..])?;
        self.cells.truncate(base);
        Ok(len)
    }
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

fn produce(pending: &mut Option<u32>, row: u32) {
    assert!(
        pending.replace(row).is_none(),
        "a value replaced another value that nothing consumed"
    );
}

fn widen(frame: &mut Frame, start: u32, end: u32) {
    if frame.base == MISSING {
        frame.base = start;
        frame.end = end;
        return;
    }
    frame.base = frame.base.min(start);
    frame.end = frame.end.max(end);
}

/// Tree-sitter stores byte offsets as `u32`.
fn byte_range(node: &Node<'_>) -> (u32, u32) {
    let start = u32::try_from(node.start_byte()).expect("Tree-sitter byte offsets fit u32");
    let end = u32::try_from(node.end_byte()).expect("Tree-sitter byte offsets fit u32");
    (start, end)
}

/// Row, edge and node indices are `u32`; `u32::MAX` is reserved.
fn index(len: usize) -> Result<u32, LimitExceeded> {
    match u32::try_from(len) {
        Ok(index) if index < PENDING => Ok(index),
        _ => Err(LimitExceeded::Allocation {
            bytes: u64::try_from(len).unwrap_or(u64::MAX),
        }),
    }
}
