//! Extract inspection joins from a winning VM effect log.

use serde::Serialize;
use tree_sitter::Node;

use crate::bytecode::{Module, SpanKind};

use plotnik_rt::RuntimeEffect;

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub v: u32,
    pub entries: Vec<InspectionEntry>,
}

#[derive(Debug, Serialize)]
pub struct InspectionEntry {
    pub span_id: u16,
    pub parent: Option<u32>,
    pub hull: Option<(u32, u32)>,
    pub bindings: Vec<Binding>,
    pub effect_range: (u32, u32),
}

#[derive(Debug, Serialize)]
pub struct Binding {
    /// JSON-pointer-style path of the bound value, relative to the builder
    /// frames open when the binding effect fired. Not absolute: a container
    /// assigned after its close (`Set`/`Push` following `ArrayClose` etc.)
    /// binds on its own entry, while the elements bound under indices on the
    /// entries active during construction — a consumer absolutizes by joining
    /// paths down the entry parent chain.
    pub path: String,
    pub effect_idx: u32,
}

enum Frame {
    Array {
        len: u32,
        provenance: ValueProvenance,
    },
    Struct {
        provenance: ValueProvenance,
    },
    Enum {
        provenance: ValueProvenance,
    },
}

struct ScalarProvenance {
    item_owner: Option<u32>,
    hull: Option<(u32, u32)>,
}

#[derive(Clone, Copy)]
struct ValueProvenance {
    item_owner: Option<u32>,
    hull: Option<(u32, u32)>,
}

impl ValueProvenance {
    fn is_unowned_absence(self) -> bool {
        self.item_owner.is_none() && self.hull.is_none()
    }
}

pub fn extract_inspection(effects: &[RuntimeEffect<'_>], module: &Module) -> Inspection {
    let extractor = Inspector::new(module);
    extractor.extract(effects)
}

struct Inspector<'m> {
    member_names: Box<[&'m str]>,
    span_kinds: Box<[SpanKind]>,
    span_members: Box<[u16]>,
    entries: Vec<InspectionEntry>,
    open: Vec<u32>,
    frames: Vec<Frame>,
    scalar_frames: Vec<ScalarProvenance>,
    pending: Option<ValueProvenance>,
}

impl<'m> Inspector<'m> {
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

    fn extract(mut self, effects: &[RuntimeEffect<'_>]) -> Inspection {
        for (effect_idx, effect) in effects.iter().enumerate() {
            let effect_idx = u32::try_from(effect_idx).expect("effect log index fits in u32");
            match effect {
                RuntimeEffect::SpanStart { id, node } => {
                    let parent = self.open.last().copied();
                    let idx = u32::try_from(self.entries.len())
                        .expect("inspection entry count fits in u32");
                    self.entries.push(InspectionEntry {
                        span_id: *id,
                        parent,
                        hull: node.map(node_hull),
                        bindings: Vec::new(),
                        effect_range: (effect_idx, effect_idx),
                    });
                    self.open.push(idx);
                    self.record_scalar_item_owner(idx);
                }
                RuntimeEffect::SpanEnd(id) => self.close_span(*id, effect_idx),
                RuntimeEffect::Node(node) => {
                    self.pending = Some(ValueProvenance {
                        item_owner: self.open.last().copied(),
                        hull: Some(node_hull(*node)),
                    });
                    if let Some(entry) = self.current_entry_mut() {
                        union_hull(&mut entry.hull, Some(node_hull(*node)));
                    }
                }
                RuntimeEffect::ArrayOpen => {
                    let provenance = self.open_value();
                    self.frames.push(Frame::Array { len: 0, provenance });
                }
                RuntimeEffect::Push => self.bind_push(effect_idx),
                RuntimeEffect::ArrayClose => match self.frames.pop() {
                    Some(Frame::Array { provenance, .. }) => self.pending = Some(provenance),
                    other => panic!(
                        "ArrayClose expects Array on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::StructOpen => {
                    let provenance = self.open_value();
                    self.frames.push(Frame::Struct { provenance });
                }
                RuntimeEffect::Set(member) => self.bind_set(*member, effect_idx),
                RuntimeEffect::StructClose => match self.frames.pop() {
                    Some(Frame::Struct { provenance }) => self.pending = Some(provenance),
                    other => panic!(
                        "StructClose expects Struct on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::EnumOpen(member) => self.open_enum(*member, effect_idx),
                RuntimeEffect::EnumClose => match self.frames.pop() {
                    Some(Frame::Enum { provenance }) => self.pending = Some(provenance),
                    other => panic!(
                        "EnumClose expects Enum on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::Null => {
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        hull: None,
                    });
                }
                RuntimeEffect::NodeStr(node) | RuntimeEffect::NodeBool(node) => {
                    let hull = Some(node_hull(*node));
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        hull,
                    });
                    if let Some(entry) = self.current_entry_mut() {
                        union_hull(&mut entry.hull, hull);
                    }
                }
                RuntimeEffect::BoolValue(_) => {
                    self.pending = Some(ValueProvenance {
                        item_owner: None,
                        hull: None,
                    });
                }
                RuntimeEffect::ScalarOpen => self.scalar_frames.push(ScalarProvenance {
                    item_owner: None,
                    hull: None,
                }),
                RuntimeEffect::ScalarMark(node) => {
                    let hull = Some(node_hull(*node));
                    for scalar in &mut self.scalar_frames {
                        union_hull(&mut scalar.hull, hull);
                    }
                    if let Some(entry) = self.current_entry_mut() {
                        union_hull(&mut entry.hull, hull);
                    }
                }
                RuntimeEffect::StrClose | RuntimeEffect::BoolClose(_) => {
                    let scalar = self
                        .scalar_frames
                        .pop()
                        .expect("scalar effect frames are balanced on the winning path");
                    self.pending = Some(ValueProvenance {
                        item_owner: scalar.item_owner,
                        hull: scalar.hull,
                    });
                }
            }
        }

        assert!(
            self.open.is_empty(),
            "inspection span stack must be empty after effect log"
        );
        Inspection {
            v: 1,
            entries: self.entries,
        }
    }

    fn close_span(&mut self, id: u16, effect_idx: u32) {
        let closed = self
            .open
            .pop()
            .expect("span brackets are balanced on the winning path");
        let closed_idx = usize::try_from(closed).expect("inspection entry index fits usize");
        let hull = {
            let entry = self
                .entries
                .get_mut(closed_idx)
                .expect("open span index addresses an inspection entry");
            assert_eq!(entry.span_id, id, "span bracket ids must pair");
            entry.effect_range.1 = effect_idx;
            entry.hull
        };

        if let Some(&parent) = self.open.last() {
            let parent_idx = usize::try_from(parent).expect("inspection entry index fits usize");
            union_hull(&mut self.entries[parent_idx].hull, hull);
        }
    }

    fn bind_push(&mut self, effect_idx: u32) {
        let Some(Frame::Array { len, .. }) = self.frames.last() else {
            panic!("Push expects Array on inspection frame stack");
        };
        let index = *len;
        let mut path = path_for_frames(&self.frames[..self.frames.len() - 1]);
        push_segment(&mut path, &index.to_string());
        let provenance = self.pending.take();
        if !provenance.is_some_and(ValueProvenance::is_unowned_absence) {
            if let Some(owner) = provenance.and_then(|value| value.item_owner) {
                self.bind_entry(owner, path, effect_idx);
            } else {
                self.bind_current(path, effect_idx);
            }
        }

        let Some(Frame::Array {
            len,
            provenance: array,
        }) = self.frames.last_mut()
        else {
            unreachable!("array frame was checked before binding Push");
        };
        if let Some(value) = provenance {
            union_hull(&mut array.hull, value.hull);
        }
        *len += 1;
    }

    fn bind_set(&mut self, member: u16, effect_idx: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, self.resolve_member_name(member));
        let provenance = self.pending.take();
        if let Some(value) = provenance {
            self.record_child_hull(value.hull);
        }
        if let Some(owner) = self.open_capture_for_member(member) {
            self.bind_entry(owner, path, effect_idx);
            return;
        }
        if provenance.is_some_and(ValueProvenance::is_unowned_absence) {
            return;
        }
        self.bind_current(path, effect_idx);
    }

    fn open_enum(&mut self, _member: u16, effect_idx: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, "$tag");
        self.bind_current(path, effect_idx);
        let provenance = self.open_value();
        self.frames.push(Frame::Enum { provenance });
    }

    fn bind_current(&mut self, path: String, effect_idx: u32) {
        let Some(&entry) = self.open.last() else {
            return;
        };
        self.bind_entry(entry, path, effect_idx);
    }

    fn bind_entry(&mut self, entry: u32, path: String, effect_idx: u32) {
        let entry = usize::try_from(entry).expect("inspection entry index fits usize");
        let entry = self
            .entries
            .get_mut(entry)
            .expect("open span index addresses an inspection entry");
        entry.bindings.push(Binding { path, effect_idx });
    }

    fn open_capture_for_member(&self, member: u16) -> Option<u32> {
        self.open.iter().rev().copied().find(|&entry| {
            let entry = usize::try_from(entry).expect("inspection entry index fits usize");
            let span_id = self.entries[entry].span_id as usize;
            self.span_kinds[span_id] == SpanKind::Capture && self.span_members[span_id] == member
        })
    }

    fn record_scalar_item_owner(&mut self, entry: u32) {
        let entry_idx = usize::try_from(entry).expect("inspection entry index fits usize");
        let span_id = self.entries[entry_idx].span_id as usize;
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
            hull: None,
        }
    }

    fn record_child_hull(&mut self, hull: Option<(u32, u32)>) {
        let Some(frame) = self.frames.last_mut() else {
            return;
        };
        let provenance = match frame {
            Frame::Array { provenance, .. }
            | Frame::Struct { provenance }
            | Frame::Enum { provenance } => provenance,
        };
        union_hull(&mut provenance.hull, hull);
    }

    fn current_entry_mut(&mut self) -> Option<&mut InspectionEntry> {
        let idx = *self.open.last()?;
        let idx = usize::try_from(idx).expect("inspection entry index fits usize");
        Some(
            self.entries
                .get_mut(idx)
                .expect("open span index addresses an inspection entry"),
        )
    }

    fn resolve_member_name(&self, idx: u16) -> &'m str {
        self.member_names[idx as usize]
    }
}

fn node_hull(node: Node<'_>) -> (u32, u32) {
    (
        u32::try_from(node.start_byte()).expect("node start byte fits in u32"),
        u32::try_from(node.end_byte()).expect("node end byte fits in u32"),
    )
}

fn union_hull(target: &mut Option<(u32, u32)>, hull: Option<(u32, u32)>) {
    let Some((start, end)) = hull else {
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
            Frame::Array { len, .. } => push_segment(&mut path, &len.to_string()),
            Frame::Struct { .. } => {}
            Frame::Enum { .. } => push_segment(&mut path, "$data"),
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
        Frame::Array { .. } => "Array",
        Frame::Struct { .. } => "Struct",
        Frame::Enum { .. } => "Enum",
    })
}
