//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! Member indices use deferred resolution via `MemberRef`.

use std::collections::BTreeMap;
use std::num::NonZeroU16;

use super::effects::{EffectOp, EffectOpcode};
use super::instructions::{
    Call, Opcode, Return, StepAddr, StepId, Trampoline, select_match_opcode,
};
use super::nav::Nav;
use crate::analyze::type_check::TypeId;

/// Node type constraint for Match instructions.
///
/// Distinguishes between named nodes (`(identifier)`), anonymous nodes (`"text"`),
/// and wildcards (`_`, `(_)`). Encoded in bytecode header byte bits 5-4.
///
/// | `node_kind` | Value | Meaning      | `node_type=0`       | `node_type>0`     |
/// | ----------- | ----- | ------------ | ------------------- | ----------------- |
/// | `00`        | Any   | `_` pattern  | No check            | (invalid)         |
/// | `01`        | Named | `(_)`/`(t)`  | Check `is_named()`  | Check `kind_id()` |
/// | `10`        | Anon  | `"text"`     | Check `!is_named()` | Check `kind_id()` |
/// | `11`        | -     | Reserved     | Error               | Error             |
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum NodeTypeIR {
    /// Any node (`_` pattern) - no type check performed.
    #[default]
    Any,
    /// Named node constraint (`(_)` or `(identifier)`).
    /// - `None` = any named node (check `is_named()`)
    /// - `Some(id)` = specific named type (check `kind_id()`)
    Named(Option<NonZeroU16>),
    /// Anonymous node constraint (`"text"` literals).
    /// - `None` = any anonymous node (check `!is_named()`)
    /// - `Some(id)` = specific anonymous type (check `kind_id()`)
    Anonymous(Option<NonZeroU16>),
}

impl NodeTypeIR {
    /// Encode to bytecode: returns (node_kind bits, node_type value).
    ///
    /// `node_kind` is 2 bits for header byte bits 5-4.
    /// `node_type` is u16 for bytes 2-3.
    pub fn to_bytes(self) -> (u8, u16) {
        match self {
            Self::Any => (0b00, 0),
            Self::Named(opt) => (0b01, opt.map(|n| n.get()).unwrap_or(0)),
            Self::Anonymous(opt) => (0b10, opt.map(|n| n.get()).unwrap_or(0)),
        }
    }

    /// Decode from bytecode: node_kind bits (2 bits) and node_type value (u16).
    pub fn from_bytes(node_kind: u8, node_type: u16) -> Self {
        match node_kind {
            0b00 => Self::Any,
            0b01 => Self::Named(NonZeroU16::new(node_type)),
            0b10 => Self::Anonymous(NonZeroU16::new(node_type)),
            _ => panic!("invalid node_kind: {node_kind}"),
        }
    }

    /// Check if this represents a specific type ID (not a wildcard).
    pub fn type_id(&self) -> Option<NonZeroU16> {
        match self {
            Self::Any => None,
            Self::Named(opt) | Self::Anonymous(opt) => *opt,
        }
    }

    /// Check if this is the Any wildcard.
    pub fn is_any(&self) -> bool {
        matches!(self, Self::Any)
    }

    /// Check if this is a Named constraint (wildcard or specific).
    pub fn is_named(&self) -> bool {
        matches!(self, Self::Named(_))
    }

    /// Check if this is an Anonymous constraint (wildcard or specific).
    pub fn is_anonymous(&self) -> bool {
        matches!(self, Self::Anonymous(_))
    }
}

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
        EffectOp::new(self.opcode, payload)
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
    /// Navigation command. `Epsilon` means pure control flow (no node check).
    pub nav: Nav,
    /// Node type constraint (Any = wildcard, Named/Anonymous for specific checks).
    pub node_type: NodeTypeIR,
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
            nav: Nav::Epsilon,
            node_type: NodeTypeIR::Any,
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
    pub fn node_type(mut self, t: NodeTypeIR) -> Self {
        self.node_type = t;
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
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.successors.len() <= 1;

        let opcode = if can_use_match8 {
            Opcode::Match8
        } else {
            let slots_needed = self.pre_effects.len()
                + self.neg_fields.len()
                + self.post_effects.len()
                + self.successors.len();
            select_match_opcode(slots_needed).expect("instruction too large")
        };

        let size = opcode.size();
        let mut bytes = vec![0u8; size];

        // Header byte layout: segment(2) | node_kind(2) | opcode(4)
        let (node_kind, node_type_val) = self.node_type.to_bytes();
        bytes[0] = (node_kind << 4) | (opcode as u8); // segment 0
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&node_type_val.to_le_bytes());
        let node_field_val = self.node_field.map(|n| n.get()).unwrap_or(0);
        bytes[4..6].copy_from_slice(&node_field_val.to_le_bytes());

        if opcode == Opcode::Match8 {
            let next = self
                .successors
                .first()
                .map(|&l| l.resolve(map))
                .unwrap_or(0);
            bytes[6..8].copy_from_slice(&next.to_le_bytes());
        } else {
            let pre_count = self.pre_effects.len();
            let neg_count = self.neg_fields.len();
            let post_count = self.post_effects.len();
            let succ_count = self.successors.len();

            // Validate bit-packed field limits (3 bits for counts, 6 bits for successors)
            assert!(
                pre_count <= 7,
                "pre_effects overflow: {pre_count} > 7 (use emit_match_with_cascade)"
            );
            assert!(neg_count <= 7, "neg_fields overflow: {neg_count} > 7");
            assert!(post_count <= 7, "post_effects overflow: {post_count} > 7");
            assert!(succ_count <= 63, "successors overflow: {succ_count} > 63");

            let counts = ((pre_count as u16) << 13)
                | ((neg_count as u16) << 10)
                | ((post_count as u16) << 7)
                | ((succ_count as u16) << 1);
            bytes[6..8].copy_from_slice(&counts.to_le_bytes());

            let mut offset = 8;
            for effect in &self.pre_effects {
                let resolved = effect.resolve(&lookup_member, &get_member_base);
                bytes[offset..offset + 2].copy_from_slice(&resolved.to_bytes());
                offset += 2;
            }
            for &field in &self.neg_fields {
                bytes[offset..offset + 2].copy_from_slice(&field.to_le_bytes());
                offset += 2;
            }
            for effect in &self.post_effects {
                let resolved = effect.resolve(&lookup_member, &get_member_base);
                bytes[offset..offset + 2].copy_from_slice(&resolved.to_bytes());
                offset += 2;
            }
            for &label in &self.successors {
                let addr = label.resolve(map);
                bytes[offset..offset + 2].copy_from_slice(&addr.to_le_bytes());
                offset += 2;
            }
        }

        bytes
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
    /// Create a return instruction at the given label.
    pub fn new(label: Label) -> Self {
        Self { label }
    }

    /// Serialize to bytecode bytes (no labels to resolve).
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
    /// Create a trampoline instruction.
    pub fn new(label: Label, next: Label) -> Self {
        Self { label, next }
    }

    /// Resolve labels and serialize to bytecode bytes.
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
    pub(crate) label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps (for header).
    pub(crate) total_steps: u16,
}

impl LayoutResult {
    /// Create a new layout result.
    pub fn new(label_to_step: BTreeMap<Label, StepAddr>, total_steps: u16) -> Self {
        Self {
            label_to_step,
            total_steps,
        }
    }

    /// Create an empty layout result.
    pub fn empty() -> Self {
        Self {
            label_to_step: BTreeMap::new(),
            total_steps: 0,
        }
    }

    pub fn label_to_step(&self) -> &BTreeMap<Label, StepAddr> {
        &self.label_to_step
    }
    pub fn total_steps(&self) -> u16 {
        self.total_steps
    }
}
