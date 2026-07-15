//! Reconstruct result provenance from a winning match journal.

use serde::Serialize;
use tree_sitter::Node;

use crate::bytecode::{Module, SpanKind};

use plotnik_runtime::{JournalEvent, MatchJournal};

#[derive(Debug, Serialize)]
pub struct ResultProvenance {
    pub version: u32,
    pub entries: Vec<ResultProvenanceEntry>,
}

#[derive(Debug, Serialize)]
pub struct ResultProvenanceEntry {
    pub query_span_id: u16,
    pub parent: Option<u32>,
    pub source_span: Option<(u32, u32)>,
    pub bindings: Vec<ProvenanceBinding>,
    #[serde(rename = "range")]
    pub event_range: (u32, u32),
}

#[derive(Debug, Serialize)]
pub struct ProvenanceBinding {
    /// JSON-pointer-style path of the bound value, relative to the builder
    /// frames open when the binding event fired. Not absolute: a container
    /// assigned after its close (`RecordSet`/`ArrayPush` following `ListClose` etc.)
    /// binds on its own entry, while the elements bound under indices on the
    /// entries active during construction — a consumer absolutizes by joining
    /// paths down the entry parent chain.
    pub path: String,
    pub event_index: u32,
}

enum Frame {
    List {
        len: u32,
        provenance: ValueProvenance,
    },
    Record {
        provenance: ValueProvenance,
    },
    Variant {
        provenance: ValueProvenance,
    },
}

struct ScalarProvenance {
    item_owner: Option<u32>,
    source_span: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct ValueProvenance {
    item_owner: Option<u32>,
    source_span: Option<(u32, u32)>,
}

impl ValueProvenance {
    fn is_unowned_absence(self) -> bool {
        self.item_owner.is_none() && self.source_span.is_none()
    }
}

pub fn extract_result_provenance(journal: &MatchJournal<'_>, module: &Module) -> ResultProvenance {
    let extractor = ProvenanceExtractor::new(module);
    extractor.extract(journal.as_slice())
}

struct ProvenanceExtractor<'m> {
    member_names: Box<[&'m str]>,
    span_kinds: Box<[SpanKind]>,
    span_members: Box<[u16]>,
    entries: Vec<ResultProvenanceEntry>,
    open: Vec<u32>,
    frames: Vec<Frame>,
    scalar_frames: Vec<ScalarProvenance>,
    pending: Option<ValueProvenance>,
}

impl<'m> ProvenanceExtractor<'m> {
    fn new(module: &'m Module) -> Self {
        let types = module.types();
        let strings = module.strings();
        let member_names = types.members().map(|m| strings.get(m.name_id)).collect();
        let span_kinds = module.spans().iter().map(|span| span.kind).collect();
        let span_members = module.spans().iter().map(|span| span.member).collect();
        Self {
            member_names,
            span_kinds,
            span_members,
            entries: Vec::new(),
            open: Vec::new(),
            frames: Vec::new(),
            scalar_frames: Vec::new(),
            pending: None,
        }
    }

    fn extract(mut self, events: &[JournalEvent<'_>]) -> ResultProvenance {
        for (event_index, event) in events.iter().enumerate() {
            let event_index = u32::try_from(event_index).expect("journal event index fits in u32");
            match event {
                JournalEvent::SpanStart { id, node } => {
                    let parent = self.open.last().copied();
                    let idx = u32::try_from(self.entries.len())
                        .expect("result provenance entry count fits in u32");
                    self.entries.push(ResultProvenanceEntry {
                        query_span_id: *id,
                        parent,
                        source_span: node.map(node_span),
                        bindings: Vec::new(),
                        event_range: (event_index, event_index),
                    });
                    self.open.push(idx);
                    self.record_scalar_item_owner(idx);
                }
                JournalEvent::SpanEnd(id) => self.close_span(*id, event_index),
                JournalEvent::Node(node) => {
                    self.pending = Some(ValueProvenance {
                        item_owner: self.open.last().copied(),
                        source_span: Some(node_span(*node)),
                    });
                    if let Some(entry) = self.current_entry_mut() {
                        extend_bounding_range(&mut entry.source_span, Some(node_span(*node)));
                    }
                }
                JournalEvent::ListOpen => {
                    let provenance = self.open_value();
                    self.frames.push(Frame::List { len: 0, provenance });
                }
                JournalEvent::ArrayPush => self.bind_push(event_index),
                JournalEvent::ListClose => match self.frames.pop() {
                    Some(Frame::List { provenance, .. }) => self.pending = Some(provenance),
                    other => panic!(
                        "ListClose expects List on provenance frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                JournalEvent::RecordOpen => {
                    let provenance = self.open_value();
                    self.frames.push(Frame::Record { provenance });
                }
                JournalEvent::RecordSet(member) => self.bind_set(*member, event_index),
                JournalEvent::RecordClose => match self.frames.pop() {
                    Some(Frame::Record { provenance }) => self.pending = Some(provenance),
                    other => panic!(
                        "RecordClose expects Record on provenance frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                JournalEvent::VariantOpen(member) => self.open_variant(*member, event_index),
                JournalEvent::VariantClose => match self.frames.pop() {
                    Some(Frame::Variant { provenance }) => self.pending = Some(provenance),
                    other => panic!(
                        "VariantClose expects Variant on provenance frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                JournalEvent::Absent => {
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        source_span: None,
                    });
                }
                JournalEvent::NodeStr(node) | JournalEvent::NodeBool(node) => {
                    let source_span = Some(node_span(*node));
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        source_span,
                    });
                    if let Some(entry) = self.current_entry_mut() {
                        extend_bounding_range(&mut entry.source_span, source_span);
                    }
                }
                JournalEvent::BoolValue(_) => {
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        source_span: None,
                    });
                }
                JournalEvent::ScalarOpen => self.scalar_frames.push(ScalarProvenance {
                    item_owner: None,
                    source_span: None,
                }),
                JournalEvent::ScalarMark(node) => {
                    let source_span = Some(node_span(*node));
                    for scalar in &mut self.scalar_frames {
                        extend_bounding_range(&mut scalar.source_span, source_span);
                    }
                    if let Some(entry) = self.current_entry_mut() {
                        extend_bounding_range(&mut entry.source_span, source_span);
                    }
                }
                JournalEvent::StrClose | JournalEvent::BoolClose(_) => {
                    let scalar = self
                        .scalar_frames
                        .pop()
                        .expect("scalar event frames are balanced on the winning path");
                    self.pending = Some(ValueProvenance {
                        item_owner: scalar.item_owner,
                        source_span: scalar.source_span,
                    });
                }
            }
        }

        assert!(
            self.open.is_empty(),
            "provenance span stack must be empty after the match journal"
        );
        ResultProvenance {
            version: 1,
            entries: self.entries,
        }
    }

    fn close_span(&mut self, id: u16, event_index: u32) {
        let closed = self
            .open
            .pop()
            .expect("span brackets are balanced on the winning path");
        let closed_idx = usize::try_from(closed).expect("provenance entry index fits usize");
        let source_span = {
            let entry = self
                .entries
                .get_mut(closed_idx)
                .expect("open span index addresses a provenance entry");
            assert_eq!(entry.query_span_id, id, "span bracket ids must pair");
            entry.event_range.1 = event_index
                .checked_add(1)
                .expect("journal event count fits in u32");
            entry.source_span
        };

        if let Some(&parent) = self.open.last() {
            let parent_idx = usize::try_from(parent).expect("provenance entry index fits usize");
            extend_bounding_range(&mut self.entries[parent_idx].source_span, source_span);
        }
    }

    fn bind_push(&mut self, event_index: u32) {
        let Some(Frame::List { len, .. }) = self.frames.last() else {
            panic!("ArrayPush expects List on provenance frame stack");
        };
        let index = *len;
        let mut path = path_for_frames(&self.frames[..self.frames.len() - 1]);
        push_segment(&mut path, &index.to_string());
        let provenance = self.pending.take();
        if !provenance.is_some_and(ValueProvenance::is_unowned_absence) {
            if let Some(owner) = provenance.and_then(|value| value.item_owner) {
                self.bind_entry(owner, path, event_index);
            } else {
                self.bind_current(path, event_index);
            }
        }

        let Some(Frame::List {
            len,
            provenance: list,
        }) = self.frames.last_mut()
        else {
            unreachable!("list frame was checked before binding ArrayPush");
        };
        if let Some(value) = provenance {
            extend_bounding_range(&mut list.source_span, value.source_span);
        }
        *len += 1;
    }

    fn bind_set(&mut self, member: u16, event_index: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, self.resolve_member_name(member));
        let provenance = self.pending.take();
        if let Some(value) = provenance {
            self.record_child_span(value.source_span);
        }
        if let Some(owner) = self.open_capture_for_member(member) {
            self.bind_entry(owner, path, event_index);
            return;
        }
        if provenance.is_some_and(ValueProvenance::is_unowned_absence) {
            return;
        }
        self.bind_current(path, event_index);
    }

    fn open_variant(&mut self, _member: u16, event_index: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, "$tag");
        self.bind_current(path, event_index);
        let provenance = self.open_value();
        self.frames.push(Frame::Variant { provenance });
    }

    fn bind_current(&mut self, path: String, event_index: u32) {
        let Some(&entry) = self.open.last() else {
            return;
        };
        self.bind_entry(entry, path, event_index);
    }

    fn bind_entry(&mut self, entry: u32, path: String, event_index: u32) {
        let entry = usize::try_from(entry).expect("provenance entry index fits usize");
        let entry = self
            .entries
            .get_mut(entry)
            .expect("open span index addresses a provenance entry");
        entry.bindings.push(ProvenanceBinding { path, event_index });
    }

    fn open_capture_for_member(&self, member: u16) -> Option<u32> {
        self.open.iter().rev().copied().find(|&entry| {
            let entry = usize::try_from(entry).expect("provenance entry index fits usize");
            let span_id = self.entries[entry].query_span_id as usize;
            self.span_kinds[span_id] == SpanKind::Capture && self.span_members[span_id] == member
        })
    }

    fn record_scalar_item_owner(&mut self, entry: u32) {
        let entry_idx = usize::try_from(entry).expect("provenance entry index fits usize");
        let span_id = self.entries[entry_idx].query_span_id as usize;
        if self.span_kinds[span_id] == SpanKind::Capture {
            return;
        }
        for scalar in &mut self.scalar_frames {
            if scalar.item_owner.is_none() {
                scalar.item_owner = Some(entry);
            }
        }
    }

    fn open_value(&self) -> ValueProvenance {
        ValueProvenance {
            item_owner: None,
            source_span: None,
        }
    }

    fn record_child_span(&mut self, source_span: Option<(u32, u32)>) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let provenance = match frame {
            Frame::List { provenance, .. }
            | Frame::Record { provenance }
            | Frame::Variant { provenance } => provenance,
        };
        extend_bounding_range(&mut provenance.source_span, source_span);
    }

    fn current_entry_mut(&mut self) -> Option<&mut ResultProvenanceEntry> {
        let idx = *self.open.last()?;
        let idx = usize::try_from(idx).expect("provenance entry index fits usize");
        Some(
            self.entries
                .get_mut(idx)
                .expect("open span index addresses a provenance entry"),
        )
    }

    fn resolve_member_name(&self, idx: u16) -> &'m str {
        self.member_names[idx as usize]
    }
}

fn node_span(node: Node<'_>) -> (u32, u32) {
    (
        u32::try_from(node.start_byte()).expect("node start byte fits in u32"),
        u32::try_from(node.end_byte()).expect("node end byte fits in u32"),
    )
}

fn extend_bounding_range(target: &mut Option<(u32, u32)>, span: Option<(u32, u32)>) {
    let Some((start, end)) = span else {
        return;
    };
    match target {
        Some((current_start, current_end)) => {
            *current_start = (*current_start).min(start);
            *current_end = (*current_end).max(end);
        }
        None => *target = Some((start, end)),
    }
}

fn path_for_frames(frames: &[Frame]) -> String {
    let mut path = String::new();
    for frame in frames {
        match frame {
            Frame::List { len, .. } => push_segment(&mut path, &len.to_string()),
            Frame::Record { .. } => {}
            Frame::Variant { .. } => push_segment(&mut path, "$data"),
        }
    }
    path
}

fn push_segment(path: &mut String, segment: &str) {
    path.push('/');
    for ch in segment.chars() {
        match ch {
            '~' => path.push_str("~0"),
            '/' => path.push_str("~1"),
            _ => path.push(ch),
        }
    }
}

fn frame_kind(frame: Option<&Frame>) -> Option<&'static str> {
    frame.map(|frame| match frame {
        Frame::List { .. } => "List",
        Frame::Record { .. } => "Record",
        Frame::Variant { .. } => "Variant",
    })
}
