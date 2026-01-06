//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! Member indices use deferred resolution via `MemberRef`.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use super::effects::{EffectOp, EffectOpcode};
use super::instructions::{Call, Match, Return, StepAddr, StepId, Trampoline, select_match_opcode};
use super::nav::Nav;
use crate::analyze::type_check::TypeId;

/// Symbolic reference, resolved to step address at layout time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Label(pub u32);

impl Label {
    /// Resolve this label to a step address using the layout mapping.
    #[inline]
    pub fn resolve(self, map: &BTreeMap<Label, StepAddr>) -> StepAddr {
        *map.get(&self).expect("label not in layout")
    }
}

/// Symbolic reference to a struct field or enum variant.
/// Resolved to absolute member index during bytecode emission.
///
/// Struct field indices are deduplicated globally: same (name, type) pair → same index.
/// This enables call-site scoping where uncaptured refs share the caller's scope.
///
/// Enum variant indices use the traditional (parent_type, relative_index) approach
/// since enum variants don't bubble between scopes.
#[derive(Clone, Copy, Debug)]
pub enum MemberRef {
    /// Already resolved to absolute index (for cases where it's known).
    Absolute(u16),
    /// Deferred resolution by field identity (for struct fields).
    /// The same (field_name, field_type) pair resolves to the same member index
    /// regardless of which struct type contains it.
    Deferred {
        /// The Symbol of the field name (from query interner).
        field_name: plotnik_core::Symbol,
        /// The TypeId of the field's value type (from query TypeContext).
        field_type: TypeId,
    },
    /// Deferred resolution by parent type + relative index (for enum variants).
    /// Uses the parent enum's member_base + relative_index.
    DeferredByIndex {
        /// The TypeId of the parent enum type.
        parent_type: TypeId,
        /// Relative index within the parent type's members.
        relative_index: u16,
    },
}

impl MemberRef {
    /// Create an absolute reference.
    pub fn absolute(index: u16) -> Self {
        Self::Absolute(index)
    }

    /// Create a deferred reference by field identity (for struct fields).
    pub fn deferred(field_name: plotnik_core::Symbol, field_type: TypeId) -> Self {
        Self::Deferred {
            field_name,
            field_type,
        }
    }

    /// Create a deferred reference by parent type + index (for enum variants).
    pub fn deferred_by_index(parent_type: TypeId, relative_index: u16) -> Self {
        Self::DeferredByIndex {
            parent_type,
            relative_index,
        }
    }

    /// Resolve this reference using lookup functions.
    ///
    /// - `lookup_member`: maps (field_name Symbol, field_type TypeId) to member index
    /// - `get_member_base`: maps parent TypeId to member base index
    pub fn resolve<F, G>(self, lookup_member: F, get_member_base: G) -> u16
    where
        F: Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
        G: Fn(TypeId) -> Option<u16>,
    {
        match self {
            Self::Absolute(n) => n,
            Self::Deferred {
                field_name,
                field_type,
            } => lookup_member(field_name, field_type).unwrap_or(0),
            Self::DeferredByIndex {
                parent_type,
                relative_index,
            } => get_member_base(parent_type).unwrap_or(0) + relative_index,
        }
    }
}

/// Effect operation with symbolic member references.
/// Used during compilation; resolved to EffectOp during emission.
#[derive(Clone, Debug)]
pub struct EffectIR {
    pub opcode: EffectOpcode,
    /// Payload for effects that don't use member indices.
    pub payload: usize,
    /// Member reference for Set/E effects (None for other effects).
    pub member_ref: Option<MemberRef>,
}

impl EffectIR {
    /// Create a simple effect without member reference.
    pub fn simple(opcode: EffectOpcode, payload: usize) -> Self {
        Self {
            opcode,
            payload,
            member_ref: None,
        }
    }

    /// Create an effect with a member reference.
    pub fn with_member(opcode: EffectOpcode, member_ref: MemberRef) -> Self {
        Self {
            opcode,
            payload: 0,
            member_ref: Some(member_ref),
        }
    }

    /// Capture current node value.
    pub fn node() -> Self {
        Self::simple(EffectOpcode::Node, 0)
    }

    /// Capture current node text.
    pub fn text() -> Self {
        Self::simple(EffectOpcode::Text, 0)
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

    /// Resolve this IR effect to a concrete EffectOp.
    ///
    /// - `lookup_member`: maps (field_name Symbol, field_type TypeId) to member index
    /// - `get_member_base`: maps parent TypeId to member base index
    pub fn resolve<F, G>(&self, lookup_member: F, get_member_base: G) -> EffectOp
    where
        F: Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
        G: Fn(TypeId) -> Option<u16>,
    {
        let payload = if let Some(member_ref) = self.member_ref {
            member_ref.resolve(&lookup_member, &get_member_base) as usize
        } else {
            self.payload
        };
        EffectOp {
            opcode: self.opcode,
            payload,
        }
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
    /// Get the label where this instruction lives.
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
    pub fn successors(&self) -> Vec<Label> {
        match self {
            Self::Match(m) => m.successors.clone(),
            Self::Call(c) => vec![c.next],
            Self::Return(_) => vec![],
            Self::Trampoline(t) => vec![t.next],
        }
    }

    /// Resolve labels and serialize to bytecode bytes.
    ///
    /// - `lookup_member`: maps (field_name Symbol, field_type TypeId) to member index
    /// - `get_member_base`: maps parent TypeId to member base index
    pub fn resolve<F, G>(
        &self,
        map: &BTreeMap<Label, StepAddr>,
        lookup_member: F,
        get_member_base: G,
    ) -> Vec<u8>
    where
        F: Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
        G: Fn(TypeId) -> Option<u16>,
    {
        match self {
            Self::Match(m) => m.resolve(map, lookup_member, get_member_base),
            Self::Call(c) => c.resolve(map).to_vec(),
            Self::Return(r) => r.resolve().to_vec(),
            Self::Trampoline(t) => t.resolve(map).to_vec(),
        }
    }
}

/// Match instruction IR with symbolic successors.
#[derive(Clone, Debug)]
pub struct MatchIR {
    /// Where this instruction lives.
    pub label: Label,
    /// Navigation command.
    pub nav: Nav,
    /// Node type constraint (None = wildcard).
    pub node_type: Option<NonZeroU16>,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NonZeroU16>,
    /// Effects to execute before match attempt.
    pub pre_effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub post_effects: Vec<EffectIR>,
    /// Successor labels (empty = accept, 1 = linear, 2+ = branch).
    pub successors: Vec<Label>,
}

impl MatchIR {
    /// Create a terminal/accept state (empty successors).
    pub fn terminal(label: Label) -> Self {
        Self {
            label,
            nav: Nav::Stay,
            node_type: None,
            node_field: None,
            pre_effects: vec![],
            neg_fields: vec![],
            post_effects: vec![],
            successors: vec![],
        }
    }

    /// Start building a match instruction at the given label.
    pub fn at(label: Label) -> Self {
        Self::terminal(label)
    }

    /// Create an epsilon transition (no node interaction) to a single successor.
    pub fn epsilon(label: Label, next: Label) -> Self {
        Self::at(label).next(next)
    }

    /// Set the navigation command.
    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    /// Set the node type constraint.
    pub fn node_type(mut self, t: impl Into<Option<NonZeroU16>>) -> Self {
        self.node_type = t.into();
        self
    }

    /// Set the field constraint.
    pub fn node_field(mut self, f: impl Into<Option<NonZeroU16>>) -> Self {
        self.node_field = f.into();
        self
    }

    /// Add a negated field constraint.
    pub fn neg_field(mut self, f: u16) -> Self {
        self.neg_fields.push(f);
        self
    }

    /// Add a pre-match effect.
    pub fn pre_effect(mut self, e: EffectIR) -> Self {
        self.pre_effects.push(e);
        self
    }

    /// Add a post-match effect.
    pub fn post_effect(mut self, e: EffectIR) -> Self {
        self.post_effects.push(e);
        self
    }

    /// Add multiple negated field constraints.
    pub fn neg_fields(mut self, fields: impl IntoIterator<Item = u16>) -> Self {
        self.neg_fields.extend(fields);
        self
    }

    /// Add multiple pre-match effects.
    pub fn pre_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        self.pre_effects.extend(effects);
        self
    }

    /// Add multiple post-match effects.
    pub fn post_effects(mut self, effects: impl IntoIterator<Item = EffectIR>) -> Self {
        self.post_effects.extend(effects);
        self
    }

    /// Set a single successor.
    pub fn next(mut self, s: Label) -> Self {
        self.successors = vec![s];
        self
    }

    /// Set multiple successors (for branches).
    pub fn next_many(mut self, s: Vec<Label>) -> Self {
        self.successors = s;
        self
    }

    /// Compute instruction size in bytes.
    pub fn size(&self) -> usize {
        // Match8 can be used if: no effects, no neg_fields, and at most 1 successor
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.successors.len() <= 1;

        if can_use_match8 {
            return 8;
        }

        // Extended match: count all payload slots
        let slots = self.pre_effects.len()
            + self.neg_fields.len()
            + self.post_effects.len()
            + self.successors.len();

        select_match_opcode(slots).map(|op| op.size()).unwrap_or(64)
    }

    /// Resolve labels and serialize to bytecode bytes.
    ///
    /// - `lookup_member`: maps (field_name Symbol, field_type TypeId) to member index
    /// - `get_member_base`: maps parent TypeId to member base index
    pub fn resolve<F, G>(
        &self,
        map: &BTreeMap<Label, StepAddr>,
        lookup_member: F,
        get_member_base: G,
    ) -> Vec<u8>
    where
        F: Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
        G: Fn(TypeId) -> Option<u16>,
    {
        let successors: Vec<StepId> = self
            .successors
            .iter()
            .map(|&l| StepId::new(l.resolve(map)))
            .collect();

        // Resolve effect member references to absolute indices
        let pre_effects: Vec<EffectOp> = self
            .pre_effects
            .iter()
            .map(|e| e.resolve(&lookup_member, &get_member_base))
            .collect();
        let post_effects: Vec<EffectOp> = self
            .post_effects
            .iter()
            .map(|e| e.resolve(&lookup_member, &get_member_base))
            .collect();

        let m = Match {
            segment: 0,
            nav: self.nav,
            node_type: self.node_type,
            node_field: self.node_field,
            pre_effects,
            neg_fields: self.neg_fields.clone(),
            post_effects,
            successors,
        };

        m.to_bytes().expect("instruction too large")
    }

    /// Check if this is an epsilon transition (no node interaction).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Stay && self.node_type.is_none() && self.node_field.is_none()
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

    /// Set the navigation command.
    pub fn nav(mut self, nav: Nav) -> Self {
        self.nav = nav;
        self
    }

    /// Set the field constraint.
    pub fn node_field(mut self, f: impl Into<Option<NonZeroU16>>) -> Self {
        self.node_field = f.into();
        self
    }

    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
        let c = Call {
            segment: 0,
            nav: self.nav,
            node_field: self.node_field,
            next: StepId::new(self.next.resolve(map)),
            target: StepId::new(self.target.resolve(map)),
        };
        c.to_bytes()
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
    /// Create a return instruction at the given label.
    pub fn new(label: Label) -> Self {
        Self { label }
    }

    /// Serialize to bytecode bytes (no labels to resolve).
    pub fn resolve(&self) -> [u8; 8] {
        let r = Return { segment: 0 };
        r.to_bytes()
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
    /// Create a trampoline instruction.
    pub fn new(label: Label, next: Label) -> Self {
        Self { label, next }
    }

    /// Resolve labels and serialize to bytecode bytes.
    pub fn resolve(&self, map: &BTreeMap<Label, StepAddr>) -> [u8; 8] {
        let t = Trampoline {
            segment: 0,
            next: StepId::new(self.next.resolve(map)),
        };
        t.to_bytes()
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
    pub label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps (for header).
    pub total_steps: u16,
}
