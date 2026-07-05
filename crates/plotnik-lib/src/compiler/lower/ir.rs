//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! A `MemberRef` stores a parent type plus a relative index, resolved to an
//! absolute member index at emit time.

use std::collections::BTreeMap;

use crate::bytecode::{EffectKind, Nav, PredicateOp, StepAddr, select_match_opcode};
use indexmap::IndexMap;

use crate::compiler::ids::{DefId, TypeId};
use crate::compiler::lower::spans::SpanTable;
use crate::core::NodeFieldId;

/// Node kind constraint for Match instructions.
///
/// The bytecode crate owns this type; re-exported here so IR producers and
/// consumers can name it as `ir::NodeKindConstraint`.
pub(crate) use crate::bytecode::NodeKindConstraint;

/// Symbolic reference, resolved to step address at layout time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Label(pub u32);

impl Label {
    #[inline]
    pub fn resolve(self, map: &BTreeMap<Label, StepAddr>) -> StepAddr {
        *map.get(&self).expect("label not in layout")
    }
}

/// Label to continue at after a callee returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnAddr(pub Label);

/// Label where a callee definition starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalleeEntry(pub Label);

/// Symbolic reference to a struct field or enum variant.
///
/// Resolved to an absolute member index at emit time: the parent type's member
/// base (`member_base`) plus `relative_index`. The parent type is a scope or
/// enum that an entrypoint result reaches, so it is always present in the emitted
/// type table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MemberRef {
    /// The query type whose member table this indexes (struct or enum).
    pub parent_type: TypeId,
    /// Relative index within the parent type's members.
    pub relative_index: u16,
}

impl MemberRef {
    pub fn new(parent_type: TypeId, relative_index: u16) -> Self {
        Self {
            parent_type,
            relative_index,
        }
    }
}

/// Effect operation with symbolic member references.
/// Used during compilation; resolved to Effect during emission.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectIR {
    kind: EffectKind,
    payload: EffectArg,
}

/// An effect's argument: a literal value, or a symbolic member reference — used by
/// Set/Enum effects — resolved to a member index during emission.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EffectArg {
    Literal(usize),
    Member(MemberRef),
}

impl EffectIR {
    /// The effect's kind.
    #[inline]
    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    /// Create a literal effect without member reference.
    pub fn literal(kind: EffectKind, payload: usize) -> Self {
        Self {
            kind,
            payload: EffectArg::Literal(payload),
        }
    }

    pub fn with_member(kind: EffectKind, member_ref: MemberRef) -> Self {
        Self {
            kind,
            payload: EffectArg::Member(member_ref),
        }
    }

    /// Capture current node value.
    pub fn node() -> Self {
        Self::literal(EffectKind::Node, 0)
    }

    /// Push null value.
    pub fn null() -> Self {
        Self::literal(EffectKind::Null, 0)
    }

    /// Push accumulated value to array.
    pub fn push() -> Self {
        Self::literal(EffectKind::Push, 0)
    }

    /// Begin array scope.
    pub fn start_arr() -> Self {
        Self::literal(EffectKind::ArrayOpen, 0)
    }

    /// End array scope.
    pub fn end_arr() -> Self {
        Self::literal(EffectKind::ArrayClose, 0)
    }

    /// Begin struct scope.
    pub fn start_struct() -> Self {
        Self::literal(EffectKind::StructOpen, 0)
    }

    /// End struct scope.
    pub fn end_struct() -> Self {
        Self::literal(EffectKind::StructClose, 0)
    }

    /// End enum scope.
    pub fn end_enum() -> Self {
        Self::literal(EffectKind::EnumClose, 0)
    }

    /// Begin suppression (suppress effects within).
    pub fn suppress_begin() -> Self {
        Self::literal(EffectKind::SuppressBegin, 0)
    }

    /// End suppression.
    pub fn suppress_end() -> Self {
        Self::literal(EffectKind::SuppressEnd, 0)
    }

    /// Open an inspection span and snapshot the current cursor node.
    #[allow(dead_code)]
    pub fn span_start_at(id: u16) -> Self {
        Self::literal(EffectKind::SpanStartAt, id as usize)
    }

    /// Open an inspection span without reading the cursor.
    pub fn span_start(id: u16) -> Self {
        Self::literal(EffectKind::SpanStart, id as usize)
    }

    /// Close an inspection span.
    pub fn span_end(id: u16) -> Self {
        Self::literal(EffectKind::SpanEnd, id as usize)
    }

    /// Whether this effect is an inspection span bracket.
    pub fn is_span_marker(&self) -> bool {
        matches!(
            self.kind(),
            EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd
        )
    }

    #[inline]
    pub fn payload(&self) -> &EffectArg {
        &self.payload
    }
}

/// Predicate value: string or regex pattern.
///
/// Both variants store predicate text. Emit interns that text into the bytecode
/// string table after the IR is complete.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PredicateValueIR {
    /// String comparison value.
    String(Box<str>),
    /// Regex pattern, compiled to a DFA during emit.
    Regex(Box<str>),
}

impl PredicateValueIR {
    pub fn text(&self) -> &str {
        match self {
            Self::String(text) | Self::Regex(text) => text,
        }
    }

    pub fn is_regex(&self) -> bool {
        matches!(self, Self::Regex(_))
    }
}

/// Predicate IR for node text filtering.
///
/// Applied after node kind/field matching. Compares node text against
/// a string literal or regex pattern.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PredicateIR {
    pub op: PredicateOp,
    pub value: PredicateValueIR,
}

impl PredicateIR {
    pub fn string(op: PredicateOp, value: impl Into<Box<str>>) -> Self {
        Self {
            op,
            value: PredicateValueIR::String(value.into()),
        }
    }

    pub fn regex(op: PredicateOp, pattern: impl Into<Box<str>>) -> Self {
        Self {
            op,
            value: PredicateValueIR::Regex(pattern.into()),
        }
    }

    /// Returns the operator as a u8 for bytecode encoding.
    pub fn op_byte(&self) -> u8 {
        self.op.to_byte()
    }
}

/// Pre-layout instruction with symbolic references.
#[derive(Clone, Debug)]
pub enum InstructionIR {
    Match(MatchIR),
    Call(CallIR),
    Return(ReturnIR),
}

impl InstructionIR {
    #[inline]
    pub fn label(&self) -> Label {
        match self {
            Self::Match(m) => m.label,
            Self::Call(c) => c.label,
            Self::Return(r) => r.label,
        }
    }

    /// Compute instruction size in bytes (8, 16, 24, 32, 48, or 64).
    pub fn size(&self) -> usize {
        match self {
            Self::Match(m) => m.size(),
            Self::Call(_) | Self::Return(_) => 8,
        }
    }

    /// Get all successor labels (for graph building).
    pub fn successors(&self) -> &[Label] {
        match self {
            Self::Match(m) => &m.successors,
            Self::Call(c) => std::slice::from_ref(&c.next),
            Self::Return(_) => &[],
        }
    }
}

/// Match instruction IR with symbolic successors.
#[derive(Clone, Debug)]
pub struct MatchIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Navigation command. `Epsilon` means pure control flow (no node check).
    pub nav: Nav,
    /// Node kind constraint (Any = wildcard, Named/Anonymous for specific checks).
    pub node_kind: NodeKindConstraint,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NodeFieldId>,
    /// Effects to execute after a successful match, in bytecode order.
    pub effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<NodeFieldId>,
    /// Predicate for node text filtering (None = no text check).
    pub predicate: Option<PredicateIR>,
    /// Successor labels (empty = accept, 1 = linear, 2+ = branch).
    pub successors: Vec<Label>,
}

impl MatchIR {
    /// Create a terminal/accept state (empty successors).
    pub fn terminal(label: Label) -> Self {
        Self {
            label,
            nav: Nav::Epsilon,
            node_kind: NodeKindConstraint::Any,
            node_field: None,
            effects: vec![],
            neg_fields: vec![],
            predicate: None,
            successors: vec![],
        }
    }

    /// Create an epsilon transition (no node interaction) to a single successor.
    pub fn epsilon(label: Label, next: Label) -> Self {
        Self::terminal(label).next(next)
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    pub fn node_kind(mut self, t: NodeKindConstraint) -> Self {
        self.node_kind = t;
        self
    }

    pub fn node_field(mut self, f: impl Into<Option<NodeFieldId>>) -> Self {
        self.node_field = f.into();
        self
    }

    pub fn prepend_effect(mut self, e: EffectIR) -> Self {
        self.effects.insert(0, e);
        self
    }

    pub fn append_effect(mut self, e: EffectIR) -> Self {
        self.effects.push(e);
        self
    }

    pub fn neg_fields(mut self, fields: impl IntoIterator<Item = NodeFieldId>) -> Self {
        self.neg_fields.extend(fields);
        self
    }

    pub fn prepend_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        let mut ordered = effects.into_iter().collect::<Vec<_>>();
        ordered.append(&mut self.effects);
        self.effects = ordered;
        self
    }

    pub fn append_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        self.effects.extend(effects);
        self
    }

    pub fn predicate(mut self, p: PredicateIR) -> Self {
        self.predicate = Some(p);
        self
    }

    pub fn next(mut self, s: Label) -> Self {
        self.successors = vec![s];
        self
    }

    pub fn successors(mut self, s: Vec<Label>) -> Self {
        self.successors = s;
        self
    }

    pub fn size(&self) -> usize {
        // Match8 can be used if: no effects, no neg_fields, no predicate, and at most 1 successor
        let can_use_match8 = self.effects.is_empty()
            && self.neg_fields.is_empty()
            && self.predicate.is_none()
            && self.successors.len() <= 1;

        if can_use_match8 {
            return 8;
        }

        // Predicate occupies 2 slots: op_byte(u8) + is_regex(u8)|value_ref(u16).
        let predicate_slots = if self.predicate.is_some() { 2 } else { 0 };
        let slots =
            self.effects.len() + self.neg_fields.len() + predicate_slots + self.successors.len();

        select_match_opcode(slots)
            .expect("instruction fits a match opcode")
            .size()
    }

    /// Check if this is an epsilon transition (no node interaction).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Epsilon
    }
}

impl From<MatchIR> for InstructionIR {
    fn from(m: MatchIR) -> Self {
        Self::Match(m)
    }
}

/// Call instruction IR with symbolic target.
#[derive(Clone, Debug)]
pub struct CallIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Navigation to apply before jumping to target.
    pub nav: Nav,
    /// Field constraint (None = no constraint).
    pub node_field: Option<NodeFieldId>,
    /// Return address (where to continue after callee returns).
    pub next: Label,
    /// Callee entry point.
    pub target: Label,
}

impl CallIR {
    /// Create a call instruction with default nav (Stay) and no field constraint.
    pub fn new(label: Label, return_addr: ReturnAddr, callee: CalleeEntry) -> Self {
        Self {
            label,
            nav: Nav::Stay,
            node_field: None,
            next: return_addr.0,
            target: callee.0,
        }
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    pub fn node_field(mut self, f: impl Into<Option<NodeFieldId>>) -> Self {
        self.node_field = f.into();
        self
    }
}

impl From<CallIR> for InstructionIR {
    fn from(c: CallIR) -> Self {
        Self::Call(c)
    }
}

/// Return instruction IR.
#[derive(Clone, Debug)]
pub struct ReturnIR {
    /// Where this instruction lives.
    pub label: Label,
}

impl ReturnIR {
    pub fn new(label: Label) -> Self {
        Self { label }
    }
}

impl From<ReturnIR> for InstructionIR {
    fn from(r: ReturnIR) -> Self {
        Self::Return(r)
    }
}

/// Compiled query IR plus entry labels produced by the compile stage.
#[derive(Clone, Debug)]
pub struct NfaGraph {
    pub(in crate::compiler::lower) instructions: Vec<InstructionIR>,
    /// Entry labels for each definition (in definition order).
    pub(in crate::compiler::lower) def_entries: IndexMap<DefId, Label>,
    /// Entry labels for consuming-only definition bodies, emitted on demand for
    /// guarded recursive nullable calls.
    pub(in crate::compiler::lower) def_entries_consuming: IndexMap<DefId, Label>,
    /// Entry labels for each emitted entrypoint wrapper, in definition order.
    pub(in crate::compiler::lower) entrypoint_wrappers: IndexMap<DefId, Label>,
    /// Inspection span table, present iff the query was compiled with inspection.
    pub(in crate::compiler::lower) spans: Option<SpanTable>,
}

impl NfaGraph {
    pub(crate) fn instructions(&self) -> &[InstructionIR] {
        &self.instructions
    }

    pub(crate) fn entrypoint_wrappers(&self) -> &IndexMap<DefId, Label> {
        &self.entrypoint_wrappers
    }

    pub(crate) fn spans(&self) -> Option<&SpanTable> {
        self.spans.as_ref()
    }
}

/// Lowered IR admitted by the query pipeline for emission.
///
/// Raw [`NfaGraph`] stays mutable inside `compiler::lower`; emission only
/// receives this wrapper, so callers cannot hand an arbitrary pass-local IR bag to
/// the bytecode writer.
#[derive(Clone, Debug)]
pub struct LoweredNfa {
    raw: NfaGraph,
}

impl LoweredNfa {
    pub(super) fn new(raw: NfaGraph) -> Self {
        Self { raw }
    }

    pub(crate) fn raw(&self) -> &NfaGraph {
        &self.raw
    }
}
