//! Extract inspection joins from a winning VM effect log.

use serde::Serialize;
use tree_sitter::Node;

use crate::bytecode::Module;

use super::effect::RuntimeEffect;

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
    Array { len: u32 },
    Struct,
    Enum,
}

pub fn extract_inspection(effects: &[RuntimeEffect<'_>], module: &Module) -> Inspection {
    let extractor = Inspector::new(module);
    extractor.extract(effects)
}

struct Inspector<'m> {
    member_names: Box<[&'m str]>,
    entries: Vec<InspectionEntry>,
    open: Vec<u32>,
    frames: Vec<Frame>,
}

impl<'m> Inspector<'m> {
    fn new(module: &'m Module) -> Self {
        let types = module.types();
        let strings = module.strings();
        let member_names = types.members().map(|m| strings.get(m.name_id)).collect();
        Self {
            member_names,
            entries: Vec::new(),
            open: Vec::new(),
            frames: Vec::new(),
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
                }
                RuntimeEffect::SpanEnd(id) => self.close_span(*id, effect_idx),
                RuntimeEffect::Node(node) => {
                    if let Some(entry) = self.current_entry_mut() {
                        union_hull(&mut entry.hull, Some(node_hull(*node)));
                    }
                }
                RuntimeEffect::ArrayOpen => self.frames.push(Frame::Array { len: 0 }),
                RuntimeEffect::Push => self.bind_push(effect_idx),
                RuntimeEffect::ArrayClose => match self.frames.pop() {
                    Some(Frame::Array { .. }) => {}
                    other => panic!(
                        "ArrayClose expects Array on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::StructOpen => self.frames.push(Frame::Struct),
                RuntimeEffect::Set(member) => self.bind_set(*member, effect_idx),
                RuntimeEffect::StructClose => match self.frames.pop() {
                    Some(Frame::Struct) => {}
                    other => panic!(
                        "StructClose expects Struct on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::EnumOpen(member) => self.open_enum(*member, effect_idx),
                RuntimeEffect::EnumClose => match self.frames.pop() {
                    Some(Frame::Enum) => {}
                    other => panic!(
                        "EnumClose expects Enum on inspection frame stack, found {:?}",
                        frame_kind(other.as_ref())
                    ),
                },
                RuntimeEffect::Null => {}
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
        let Some(Frame::Array { len }) = self.frames.last() else {
            panic!("Push expects Array on inspection frame stack");
        };
        let index = *len;
        let mut path = path_for_frames(&self.frames[..self.frames.len() - 1]);
        push_segment(&mut path, &index.to_string());
        self.bind_current(path, effect_idx);

        let Some(Frame::Array { len }) = self.frames.last_mut() else {
            unreachable!("array frame was checked before binding Push");
        };
        *len += 1;
    }

    fn bind_set(&mut self, member: u16, effect_idx: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, self.resolve_member_name(member));
        self.bind_current(path, effect_idx);
    }

    fn open_enum(&mut self, _member: u16, effect_idx: u32) {
        let mut path = path_for_frames(&self.frames);
        push_segment(&mut path, "$tag");
        self.bind_current(path, effect_idx);
        self.frames.push(Frame::Enum);
    }

    fn bind_current(&mut self, path: String, effect_idx: u32) {
        let Some(entry) = self.current_entry_mut() else {
            return;
        };
        entry.bindings.push(Binding { path, effect_idx });
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
            Frame::Array { len } => push_segment(&mut path, &len.to_string()),
            Frame::Struct => {}
            Frame::Enum => push_segment(&mut path, "$data"),
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
        Frame::Struct => "Struct",
        Frame::Enum => "Enum",
    })
}
