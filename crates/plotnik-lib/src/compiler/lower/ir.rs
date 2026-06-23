//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! A `MemberRef` stores a parent type plus a relative index, resolved to an
//! absolute member index at emit time.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use crate::bytecode::{EffectKind, Nav, PredicateOp, StepAddr, select_match_opcode};
use indexmap::IndexMap;

use crate::compiler::core::{DefId, TypeId};

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

/// Symbolic reference to a struct field or enum variant.
///
/// Resolved to an absolute member index at emit time: the parent type's member
/// base (`get_member_base`) plus `relative_index`. The parent type is a scope or
/// enum that an entrypoint result reaches, so it is always present in the emitted
/// type table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectIR {
    kind: EffectKind,
    payload: EffectArg,
}

/// An effect's argument: a literal value, or a symbolic member reference — used by
/// Set/Enum effects — resolved to a member index during emission.
#[derive(Clone, Debug, PartialEq, Eq)]
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

    /// Begin enum scope.
    pub fn start_enum() -> Self {
        Self::literal(EffectKind::EnumOpen, 0)
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

    #[inline]
    pub fn payload(&self) -> &EffectArg {
        &self.payload
    }
}

/// Predicate value: string or regex pattern.
///
/// Both variants store predicate text. Emit interns that text into the bytecode
/// string table after the IR is complete.
#[derive(Clone, Debug, PartialEq, Eq)]
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
#[derive(Clone, Debug, PartialEq, Eq)]
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
    Trampoline(TrampolineIR),
}

impl InstructionIR {
    #[inline]
    pub fn label(&self) -> Label {
        match self {
            Self::Match(m) => m.label,
            Self::Call(c) => c.label,
            Self::Return(r) => r.label,
            Self::Trampoline(t) => t.label,
        }
    }

    /// Compute instruction size in bytes (8, 16, 24, 32, 48, or 64).
    pub fn size(&self) -> usize {
        match self {
            Self::Match(m) => m.size(),
            Self::Call(_) | Self::Return(_) | Self::Trampoline(_) => 8,
        }
    }

    /// Get all successor labels (for graph building).
    pub fn successors(&self) -> &[Label] {
        match self {
            Self::Match(m) => &m.successors,
            Self::Call(c) => std::slice::from_ref(&c.next),
            Self::Return(_) => &[],
            Self::Trampoline(t) => std::slice::from_ref(&t.next),
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
    pub node_field: Option<NonZeroU16>,
    /// Effects to execute before match attempt.
    pub pre_effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub post_effects: Vec<EffectIR>,
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
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
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

    pub fn node_field(mut self, f: impl Into<Option<NonZeroU16>>) -> Self {
        self.node_field = f.into();
        self
    }

    pub fn pre_effect(mut self, e: EffectIR) -> Self {
        self.pre_effects.push(e);
        self
    }

    pub fn post_effect(mut self, e: EffectIR) -> Self {
        self.post_effects.push(e);
        self
    }

    pub fn neg_fields(mut self, fields: impl IntoIterator<Item = u16>) -> Self {
        self.neg_fields.extend(fields);
        self
    }

    pub fn pre_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        self.pre_effects.extend(effects);
        self
    }

    pub fn post_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        self.post_effects.extend(effects);
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
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.predicate.is_none()
            && self.successors.len() <= 1;

        if can_use_match8 {
            return 8;
        }

        // Predicate occupies 2 slots: op_byte(u8) + is_regex(u8)|value_ref(u16).
        let predicate_slots = if self.predicate.is_some() { 2 } else { 0 };
        let slots = self.pre_effects.len()
            + self.neg_fields.len()
            + self.post_effects.len()
            + predicate_slots
            + self.successors.len();

        select_match_opcode(slots).map(|op| op.size()).unwrap_or(64)
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
    pub node_field: Option<NonZeroU16>,
    /// Return address (where to continue after callee returns).
    pub next: Label,
    /// Callee entry point.
    pub target: Label,
}

impl CallIR {
    /// Create a call instruction with default nav (Stay) and no field constraint.
    pub fn new(label: Label, target: Label, next: Label) -> Self {
        Self {
            label,
            nav: Nav::Stay,
            node_field: None,
            next,
            target,
        }
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    pub fn node_field(mut self, f: impl Into<Option<NonZeroU16>>) -> Self {
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

/// Trampoline instruction IR with symbolic return address.
///
/// Trampoline is like Call, but the target comes from VM context (external parameter)
/// rather than being encoded in the instruction. Used for universal entry preamble.
#[derive(Clone, Debug)]
pub struct TrampolineIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Return address (where to continue after entrypoint returns).
    pub next: Label,
}

impl TrampolineIR {
    pub fn new(label: Label, next: Label) -> Self {
        Self { label, next }
    }
}

impl From<TrampolineIR> for InstructionIR {
    fn from(t: TrampolineIR) -> Self {
        Self::Trampoline(t)
    }
}

/// Result of layout: maps labels to step addresses.
#[derive(Clone, Debug)]
pub struct LayoutMap {
    /// Mapping from symbolic labels to concrete step addresses (raw u16).
    label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps. Held as `u32` so a query whose layout overflows
    /// the `u16` step-address space is detectable at emit time instead of
    /// wrapping silently; `emit` rejects it before any address is used.
    total_steps: u32,
}

impl LayoutMap {
    pub fn new(label_to_step: BTreeMap<Label, StepAddr>, total_steps: u32) -> Self {
        Self {
            label_to_step,
            total_steps,
        }
    }

    pub fn empty() -> Self {
        Self {
            label_to_step: BTreeMap::new(),
            total_steps: 0,
        }
    }

    pub fn step_addrs(&self) -> &BTreeMap<Label, StepAddr> {
        &self.label_to_step
    }
    pub fn total_steps(&self) -> u32 {
        self.total_steps
    }
}

/// Compiled query IR plus entry labels produced by the compile stage.
#[derive(Clone, Debug)]
pub struct CompileResult {
    pub instructions: Vec<InstructionIR>,
    /// Entry labels for each definition (in definition order).
    pub def_entries: IndexMap<DefId, Label>,
    /// Entry label for the universal preamble.
    /// The preamble wraps any entrypoint: Struct -> Trampoline -> EndStruct -> Return
    pub preamble_entry: Label,
}
