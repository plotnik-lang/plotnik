use serde::Serialize;
use tree_sitter::Node;

use crate::bytecode::{Module, SpanKind};
use plotnik_runtime::{Buffer, Document, JournalEvent, LimitExceeded, Meter, Text};

#[derive(Debug, Serialize)]
pub struct ResultProvenanceEntry {
    pub query_span_id: u16,
    pub parent: Option<u32>,
    pub source_span: Option<(u32, u32)>,
    pub bindings: Buffer<ProvenanceBinding>,
    #[serde(rename = "range")]
    pub event_range: (u32, u32),
}

#[derive(Debug, Serialize)]
pub struct ProvenanceBinding {
    /// JSON-pointer path relative to the containing provenance entry.
    pub path: Text,
    pub event_index: u32,
}

#[derive(Clone, Copy)]
enum Kind {
    List,
    Record,
    Variant,
}

struct Selection {
    path: Text,
    event: u32,
    owner: Option<u32>,
}

struct Frame {
    kind: Kind,
    len: u32,
    value: Value,
    selected: Option<Selection>,
}

#[derive(Clone, Copy, Default)]
struct Value {
    owner: Option<u32>,
    span: Option<(u32, u32)>,
}

/// Extract provenance with the same operation account as the retained result.
pub fn extract_result_provenance(
    document: &Document<'_, '_, '_>,
    module: &Module,
) -> Result<Buffer<ResultProvenanceEntry>, LimitExceeded> {
    let mut meter = document.account().meter();
    let mut extractor = Extractor {
        module,
        entries: Buffer::new(),
        open: Buffer::new(),
        frames: Buffer::new(),
        scalars: Buffer::new(),
        pending: None,
    };
    for (index, event) in document.inspection_events().enumerate() {
        meter.charge(1)?;
        let index =
            u32::try_from(index).map_err(|_| LimitExceeded::Allocation { bytes: u64::MAX })?;
        extractor.event(&mut meter, event, index)?;
    }
    assert!(
        extractor.open.is_empty() && extractor.frames.is_empty() && extractor.scalars.is_empty(),
        "accepted provenance has balanced frames"
    );
    Ok(extractor.entries)
}

struct Extractor<'m> {
    module: &'m Module,
    entries: Buffer<ResultProvenanceEntry>,
    open: Buffer<u32>,
    frames: Buffer<Frame>,
    scalars: Buffer<Value>,
    pending: Option<Value>,
}

impl Extractor<'_> {
    fn event(
        &mut self,
        meter: &mut Meter<'_>,
        event: JournalEvent<'_>,
        index: u32,
    ) -> Result<(), LimitExceeded> {
        match event {
            JournalEvent::SpanStart { id, node } => {
                let entry = u32::try_from(self.entries.len())
                    .map_err(|_| LimitExceeded::Allocation { bytes: u64::MAX })?;
                self.entries.push(
                    meter,
                    ResultProvenanceEntry {
                        query_span_id: id,
                        parent: self.open.last().copied(),
                        source_span: node.map(node_span),
                        bindings: Buffer::new(),
                        event_range: (index, index),
                    },
                )?;
                self.open.push(meter, entry)?;
                if self.module.spans().get(id as usize).kind != SpanKind::Capture
                    && let Some(scalar) = self.scalars.last_mut()
                    && scalar.owner.is_none()
                {
                    scalar.owner = Some(entry);
                }
            }
            JournalEvent::SpanEnd(id) => {
                let entry = self.open.pop().expect("balanced inspection span") as usize;
                assert_eq!(
                    self.entries[entry].query_span_id, id,
                    "paired inspection spans"
                );
                self.entries[entry].event_range.1 =
                    index.checked_add(1).expect("event index fits u32");
                self.extend_current(self.entries[entry].source_span);
            }
            JournalEvent::RecordSet(member) => {
                let mut path = self.path(meter)?;
                let name = self
                    .module
                    .strings()
                    .get(self.module.types().get_member(member as usize).name_id);
                segment(meter, &mut path, name)?;
                let mut owner = None;
                for &entry in self.open.iter().rev() {
                    meter.charge(1)?;
                    let span = self
                        .module
                        .spans()
                        .get(self.entries[entry as usize].query_span_id as usize);
                    if span.kind == SpanKind::Capture && span.member == member {
                        owner = Some(entry);
                        break;
                    }
                }
                let frame = self.frames.last_mut().expect("selected field has a frame");
                assert!(
                    frame.selected.is_none(),
                    "field selection is complete before the next"
                );
                frame.selected = Some(Selection {
                    path,
                    event: index,
                    owner: owner.or_else(|| self.open.last().copied()),
                });
            }
            JournalEvent::ListOpen | JournalEvent::RecordOpen | JournalEvent::VariantOpen(_) => {
                let kind = match event {
                    JournalEvent::ListOpen => Kind::List,
                    JournalEvent::RecordOpen => Kind::Record,
                    _ => {
                        let mut path = self.path(meter)?;
                        segment(meter, &mut path, "$tag")?;
                        self.bind(meter, self.open.last().copied(), path, index)?;
                        Kind::Variant
                    }
                };
                self.frames.push(
                    meter,
                    Frame {
                        kind,
                        len: 0,
                        value: Value::default(),
                        selected: None,
                    },
                )?;
            }
            JournalEvent::ListClose | JournalEvent::RecordClose | JournalEvent::VariantClose => {
                let frame = self.frames.pop().expect("balanced provenance value");
                assert!(
                    frame.selected.is_none(),
                    "selected provenance field has a value"
                );
                self.produce(meter, frame.value)?;
            }
            JournalEvent::ArrayPush => {
                let value = self.pending.take().expect("array element follows a value");
                let path = self.path(meter)?;
                let frame = self.frames.last_mut().expect("open list");
                assert!(
                    matches!(frame.kind, Kind::List),
                    "array element belongs to list"
                );
                frame.len = frame.len.checked_add(1).expect("list count fits u32");
                extend(&mut frame.value.span, value.span);
                if value.owner.is_some() || value.span.is_some() {
                    self.bind(
                        meter,
                        value.owner.or_else(|| self.open.last().copied()),
                        path,
                        index,
                    )?;
                }
            }
            JournalEvent::ScalarOpen => self.scalars.push(meter, Value::default())?,
            JournalEvent::ScalarMark(node) => {
                extend(
                    &mut self.scalars.last_mut().expect("open scalar").span,
                    Some(node_span(node)),
                );
                self.extend_current(Some(node_span(node)));
            }
            JournalEvent::TextClose | JournalEvent::BoolClose(_) => {
                let scalar = self.scalars.pop().expect("balanced scalar");
                if let Some(parent) = self.scalars.last_mut() {
                    extend(&mut parent.span, scalar.span);
                    if parent.owner.is_none() {
                        parent.owner = scalar.owner;
                    }
                }
                self.produce(meter, scalar)?;
            }
            JournalEvent::Node(node)
            | JournalEvent::NodeText(node)
            | JournalEvent::NodeBool(node) => {
                let value = Value {
                    owner: if matches!(event, JournalEvent::Node(_)) {
                        self.open.last().copied()
                    } else {
                        None
                    },
                    span: Some(node_span(node)),
                };
                self.extend_current(value.span);
                self.produce(meter, value)?;
            }
            JournalEvent::Absent | JournalEvent::BoolValue(_) => {
                self.produce(meter, Value::default())?
            }
        }
        Ok(())
    }

    fn produce(&mut self, meter: &mut Meter<'_>, value: Value) -> Result<(), LimitExceeded> {
        if let Some(frame) = self.frames.last_mut() {
            extend(&mut frame.value.span, value.span);
            if let Some(selected) = frame.selected.take() {
                if value.owner.is_some() || value.span.is_some() {
                    self.bind(
                        meter,
                        selected.owner.or(value.owner),
                        selected.path,
                        selected.event,
                    )?;
                }
                return Ok(());
            }
        }
        self.pending = Some(value);
        Ok(())
    }

    fn bind(
        &mut self,
        meter: &mut Meter<'_>,
        owner: Option<u32>,
        path: Text,
        event_index: u32,
    ) -> Result<(), LimitExceeded> {
        if let Some(owner) = owner {
            self.entries[owner as usize]
                .bindings
                .push(meter, ProvenanceBinding { path, event_index })?;
        }
        Ok(())
    }

    fn path(&self, meter: &mut Meter<'_>) -> Result<Text, LimitExceeded> {
        let mut path = Text::new(meter.account());
        for frame in self.frames.iter() {
            meter.charge(1)?;
            match frame.kind {
                Kind::Record => {}
                Kind::Variant => segment(meter, &mut path, "$data")?,
                Kind::List => {
                    let mut digits = [0u8; 10];
                    let mut at = digits.len();
                    let mut value = frame.len;
                    loop {
                        at -= 1;
                        digits[at] = b'0' + (value % 10) as u8;
                        value /= 10;
                        if value == 0 {
                            break;
                        }
                    }
                    segment(
                        meter,
                        &mut path,
                        std::str::from_utf8(&digits[at..]).expect("ASCII index"),
                    )?;
                }
            }
        }
        Ok(path)
    }

    fn extend_current(&mut self, span: Option<(u32, u32)>) {
        if let Some(&entry) = self.open.last() {
            extend(&mut self.entries[entry as usize].source_span, span);
        }
    }
}

fn node_span(node: Node<'_>) -> (u32, u32) {
    (
        u32::try_from(node.start_byte()).expect("node offset fits u32"),
        u32::try_from(node.end_byte()).expect("node offset fits u32"),
    )
}

fn extend(target: &mut Option<(u32, u32)>, span: Option<(u32, u32)>) {
    let Some((start, end)) = span else {
        return;
    };
    *target = Some(match *target {
        Some((a, b)) => (a.min(start), b.max(end)),
        None => (start, end),
    });
}

fn segment(meter: &mut Meter<'_>, path: &mut Text, text: &str) -> Result<(), LimitExceeded> {
    meter.charge_bytes(text.len() as u64)?;
    path.push(meter, "/")?;
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        let escape = match byte {
            b'~' => "~0",
            b'/' => "~1",
            _ => continue,
        };
        path.push(meter, &text[start..index])?;
        path.push(meter, escape)?;
        start = index + 1;
    }
    path.push(meter, &text[start..])
}
