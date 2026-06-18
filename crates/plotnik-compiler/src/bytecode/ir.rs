//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! A `MemberRef` stores a parent type plus a relative index, resolved to an
//! absolute member index at emit time.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use crate::analyze::type_check::TypeId;
use crate::emit::EmitError;
use plotnik_bytecode::{
    Call, EffectOp, EffectOpcode, MatchInstr, MatchPredicate, Nav, PredicateOp, Return, StepAddr,
    StepId, Trampoline, select_match_opcode,
};

/// Resolver bundle for bytecode emission.
///
/// Bundles the deferred-reference resolvers (`get_member_base` for struct/enum
/// member bases, `lookup_regex` for predicate patterns) into one value so
/// `resolve` signatures stay flat. The resolvers borrow the emission tables;
/// build via [`EmitContext::new`].
pub struct EmitContext<'a> {
    get_member_base: &'a dyn Fn(TypeId) -> Option<u16>,
    lookup_regex: &'a dyn Fn(plotnik_bytecode::StringId) -> Option<u16>,
}

impl<'a> EmitContext<'a> {
    pub fn new(
        get_member_base: &'a dyn Fn(TypeId) -> Option<u16>,
        lookup_regex: &'a dyn Fn(plotnik_bytecode::StringId) -> Option<u16>,
    ) -> Self {
        Self {
            get_member_base,
            lookup_regex,
        }
    }
}

/// Node type constraint for Match instructions.
///
/// The bytecode crate owns this type; re-exported here so existing
/// `crate::bytecode::NodeTypeIR` references resolve unchanged.
pub use plotnik_bytecode::NodeTypeIR;

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

    pub fn resolve(self, ctx: &EmitContext) -> u16 {
        (ctx.get_member_base)(self.parent_type).expect("member base must resolve")
            + self.relative_index
    }
}

/// Effect operation with symbolic member references.
/// Used during compilation; resolved to EffectOp during emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectIR {
    opcode: EffectOpcode,
    payload: EffectPayload,
}

/// An effect's payload: a raw value, or a symbolic member reference — used by
/// Set/Enum effects — resolved to a member index during emission.
#[derive(Clone, Debug, PartialEq, Eq)]
enum EffectPayload {
    Raw(usize),
    Member(MemberRef),
}

impl EffectIR {
    /// The effect's opcode.
    #[inline]
    pub fn opcode(&self) -> EffectOpcode {
        self.opcode
    }

    /// Create a simple effect without member reference.
    pub fn simple(opcode: EffectOpcode, payload: usize) -> Self {
        Self {
            opcode,
            payload: EffectPayload::Raw(payload),
        }
    }

    pub fn with_member(opcode: EffectOpcode, member_ref: MemberRef) -> Self {
        Self {
            opcode,
            payload: EffectPayload::Member(member_ref),
        }
    }

    /// Capture current node value.
    pub fn node() -> Self {
        Self::simple(EffectOpcode::Node, 0)
    }

    /// Push null value.
    pub fn null() -> Self {
        Self::simple(EffectOpcode::Null, 0)
    }

    /// Push accumulated value to array.
    pub fn push() -> Self {
        Self::simple(EffectOpcode::Push, 0)
    }

    /// Begin array scope.
    pub fn start_arr() -> Self {
        Self::simple(EffectOpcode::Arr, 0)
    }

    /// End array scope.
    pub fn end_arr() -> Self {
        Self::simple(EffectOpcode::EndArr, 0)
    }

    /// Begin object scope.
    pub fn start_obj() -> Self {
        Self::simple(EffectOpcode::Obj, 0)
    }

    /// End object scope.
    pub fn end_obj() -> Self {
        Self::simple(EffectOpcode::EndObj, 0)
    }

    /// Begin enum scope.
    pub fn start_enum() -> Self {
        Self::simple(EffectOpcode::Enum, 0)
    }

    /// End enum scope.
    pub fn end_enum() -> Self {
        Self::simple(EffectOpcode::EndEnum, 0)
    }

    /// Begin suppression (suppress effects within).
    pub fn suppress_begin() -> Self {
        Self::simple(EffectOpcode::SuppressBegin, 0)
    }

    /// End suppression.
    pub fn suppress_end() -> Self {
        Self::simple(EffectOpcode::SuppressEnd, 0)
    }

    pub fn resolve(&self, ctx: &EmitContext) -> EffectOp {
        let payload = match &self.payload {
            EffectPayload::Member(member_ref) => member_ref.resolve(ctx) as usize,
            EffectPayload::Raw(payload) => *payload,
        };
        EffectOp::new(self.opcode, payload)
    }
}

/// Predicate value: string or regex pattern.
///
/// Both variants store StringId (index into StringTable). For regex predicates,
/// the pattern string is also compiled to a DFA during emit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PredicateValueIR {
    /// String comparison value.
    String(plotnik_bytecode::StringId),
    /// Regex pattern (StringId for pattern, compiled to DFA during emit).
    Regex(plotnik_bytecode::StringId),
}

/// Predicate IR for node text filtering.
///
/// Applied after node type/field matching. Compares node text against
/// a string literal or regex pattern.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PredicateIR {
    pub op: PredicateOp,
    pub value: PredicateValueIR,
}

impl PredicateIR {
    pub fn string(op: PredicateOp, value: plotnik_bytecode::StringId) -> Self {
        Self {
            op,
            value: PredicateValueIR::String(value),
        }
    }

    pub fn regex(op: PredicateOp, pattern_id: plotnik_bytecode::StringId) -> Self {
        Self {
            op,
            value: PredicateValueIR::Regex(pattern_id),
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

    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(
        &self,
        map: &BTreeMap<Label, StepAddr>,
        ctx: &EmitContext,
    ) -> Result<Vec<u8>, EmitError> {
        match self {
            Self::Match(m) => m.resolve(map, ctx),
            Self::Call(c) => Ok(c.resolve(map).to_vec()),
            Self::Return(r) => Ok(r.resolve().to_vec()),
            Self::Trampoline(t) => Ok(t.resolve(map).to_vec()),
        }
    }
}

/// Match instruction IR with symbolic successors.
#[derive(Clone, Debug)]
pub struct MatchIR {
    /// Where this instruction lives.
    pub(crate) label: Label,
    /// Navigation command. `Epsilon` means pure control flow (no node check).
    pub(crate) nav: Nav,
    /// Node type constraint (Any = wildcard, Named/Anonymous for specific checks).
    pub(crate) node_type: NodeTypeIR,
    /// Field constraint (None = wildcard).
    pub(crate) node_field: Option<NonZeroU16>,
    /// Effects to execute before match attempt.
    pub(crate) pre_effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub(crate) neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub(crate) post_effects: Vec<EffectIR>,
    /// Predicate for node text filtering (None = no text check).
    pub(crate) predicate: Option<PredicateIR>,
    /// Successor labels (empty = accept, 1 = linear, 2+ = branch).
    pub(crate) successors: Vec<Label>,
}

impl MatchIR {
    /// Create a terminal/accept state (empty successors).
    pub fn terminal(label: Label) -> Self {
        Self {
            label,
            nav: Nav::Epsilon,
            node_type: NodeTypeIR::Any,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            predicate: None,
            successors: vec![],
        }
    }

    pub fn at(label: Label) -> Self {
        Self::terminal(label)
    }

    /// Create an epsilon transition (no node interaction) to a single successor.
    pub fn epsilon(label: Label, next: Label) -> Self {
        Self::at(label).next(next)
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    pub fn node_type(mut self, t: NodeTypeIR) -> Self {
        self.node_type = t;
        self
    }

    pub fn node_field(mut self, f: impl Into<Option<NonZeroU16>>) -> Self {
        self.node_field = f.into();
        self
    }

    pub fn neg_field(mut self, f: u16) -> Self {
        self.neg_fields.push(f);
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

    pub fn next_many(mut self, s: Vec<Label>) -> Self {
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

    /// Resolve symbolic references and encode to bytecode bytes.
    ///
    /// Builds the owned [`MatchInstr`] the bytecode crate knows how to encode,
    /// so encode and decode live together and capacity overflows surface as
    /// [`EmitError`] instead of panicking.
    pub fn resolve(
        &self,
        map: &BTreeMap<Label, StepAddr>,
        ctx: &EmitContext,
    ) -> Result<Vec<u8>, EmitError> {
        let pre_effects = self.pre_effects.iter().map(|e| e.resolve(ctx)).collect();
        let post_effects = self.post_effects.iter().map(|e| e.resolve(ctx)).collect();
        let predicate = self.predicate.as_ref().map(|pred| {
            let is_regex = matches!(pred.value, PredicateValueIR::Regex(_));
            let value_ref = match &pred.value {
                PredicateValueIR::String(string_id) => string_id.get(),
                PredicateValueIR::Regex(string_id) => {
                    (ctx.lookup_regex)(*string_id).expect("regex predicate must be interned")
                }
            };
            MatchPredicate {
                op: pred.op_byte(),
                is_regex,
                value_ref,
            }
        });
        let successors = self
            .successors
            .iter()
            .map(|&l| StepId::new(l.resolve(map)))
            .collect();

        let instr = MatchInstr {
            nav: self.nav,
            node_type: self.node_type,
            node_field: self.node_field,
            pre_effects,
            neg_fields: self.neg_fields.clone(),
            post_effects,
            predicate,
            successors,
        };
        instr.encode().map_err(EmitError::from)
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

    pub fn resolve(&self, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
        Call::new(
            self.nav,
            self.node_field,
            StepId::new(self.next.resolve(map)),
            StepId::new(self.target.resolve(map)),
        )
        .to_bytes()
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

    pub fn resolve(&self) -> [u8; 8] {
        Return::new().to_bytes()
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

    pub fn resolve(&self, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
        Trampoline::new(StepId::new(self.next.resolve(map))).to_bytes()
    }
}

impl From<TrampolineIR> for InstructionIR {
    fn from(t: TrampolineIR) -> Self {
        Self::Trampoline(t)
    }
}

/// Result of layout: maps labels to step addresses.
#[derive(Clone, Debug)]
pub struct LayoutResult {
    /// Mapping from symbolic labels to concrete step addresses (raw u16).
    label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps. Held as `u32` so a query whose layout overflows
    /// the `u16` step-address space is detectable at emit time instead of
    /// wrapping silently; `emit` rejects it before any address is used.
    total_steps: u32,
}

impl LayoutResult {
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

    pub fn label_to_step(&self) -> &BTreeMap<Label, StepAddr> {
        &self.label_to_step
    }
    pub fn total_steps(&self) -> u32 {
        self.total_steps
    }
}
