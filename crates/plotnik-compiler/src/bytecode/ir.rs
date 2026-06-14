//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to step addresses (u16) for serialization.
//! Member indices use deferred resolution via `MemberRef`.

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
/// Bundles the three deferred-reference resolvers (`lookup_member` for struct
/// fields, `get_member_base` for enum variants, `lookup_regex` for predicate
/// patterns) into one value so `resolve` signatures stay flat. The resolvers
/// borrow the emission tables; build via [`EmitContext::from_tables`].
pub struct EmitContext<'a> {
    lookup_member: &'a dyn Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
    get_member_base: &'a dyn Fn(TypeId) -> Option<u16>,
    lookup_regex: &'a dyn Fn(plotnik_bytecode::StringId) -> Option<u16>,
}

impl<'a> EmitContext<'a> {
    /// Build a resolver context from explicit resolver functions.
    pub fn new(
        lookup_member: &'a dyn Fn(plotnik_core::Symbol, TypeId) -> Option<u16>,
        get_member_base: &'a dyn Fn(TypeId) -> Option<u16>,
        lookup_regex: &'a dyn Fn(plotnik_bytecode::StringId) -> Option<u16>,
    ) -> Self {
        Self {
            lookup_member,
            get_member_base,
            lookup_regex,
        }
    }

    /// Resolve a struct field reference to its member index.
    ///
    /// Members are deduplicated globally by field identity (name, type).
    fn lookup_member(&self, field_name: plotnik_core::Symbol, field_type: TypeId) -> Option<u16> {
        (self.lookup_member)(field_name, field_type)
    }

    /// Resolve an enum's parent type to its member base index.
    fn get_member_base(&self, parent_type: TypeId) -> Option<u16> {
        (self.get_member_base)(parent_type)
    }

    /// Resolve a regex predicate pattern to its RegexTable index.
    fn lookup_regex(&self, string_id: plotnik_bytecode::StringId) -> Option<u16> {
        (self.lookup_regex)(string_id)
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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// Resolve this reference using the emission context's lookup tables.
    pub fn resolve(self, ctx: &EmitContext) -> u16 {
        match self {
            Self::Absolute(n) => n,
            Self::Deferred {
                field_name,
                field_type,
            } => ctx
                .lookup_member(field_name, field_type)
                .expect("deferred member reference must resolve"),
            Self::DeferredByIndex {
                parent_type,
                relative_index,
            } => {
                ctx.get_member_base(parent_type)
                    .expect("deferred member base must resolve")
                    + relative_index
            }
        }
    }
}

/// Effect operation with symbolic member references.
/// Used during compilation; resolved to EffectOp during emission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectIR {
    opcode: EffectOpcode,
    /// Payload for effects that don't use member indices.
    payload: usize,
    /// Member reference for Set/E effects (None for other effects).
    member_ref: Option<MemberRef>,
}

impl EffectIR {
    /// The effect's opcode.
    #[inline]
    pub fn opcode(&self) -> EffectOpcode {
        self.opcode
    }

    /// The raw payload for effects that don't use member indices.
    #[inline]
    pub fn payload(&self) -> usize {
        self.payload
    }

    /// The member reference for Set/Enum effects (None for other effects).
    #[inline]
    pub fn member_ref(&self) -> Option<MemberRef> {
        self.member_ref
    }

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
    pub fn resolve(&self, ctx: &EmitContext) -> EffectOp {
        let payload = if let Some(member_ref) = self.member_ref {
            member_ref.resolve(ctx) as usize
        } else {
            self.payload
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
    /// Create a string predicate (==, !=, ^=, $=, *=).
    pub fn string(op: PredicateOp, value: plotnik_bytecode::StringId) -> Self {
        Self {
            op,
            value: PredicateValueIR::String(value),
        }
    }

    /// Create a regex predicate (=~, !~).
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

    /// Set the predicate for node text filtering.
    pub fn predicate(mut self, p: PredicateIR) -> Self {
        self.predicate = Some(p);
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
        // Match8 can be used if: no effects, no neg_fields, no predicate, and at most 1 successor
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.predicate.is_none()
            && self.successors.len() <= 1;

        if can_use_match8 {
            return 8;
        }

        // Extended match: count all payload slots
        // Predicate uses 2 slots: op_byte(u8) + is_regex(u8) | value_ref(u16)
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
                PredicateValueIR::Regex(string_id) => ctx
                    .lookup_regex(*string_id)
                    .expect("regex predicate must be interned"),
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
    label_to_step: BTreeMap<Label, StepAddr>,
    /// Total number of steps. Held as `u32` so a query whose layout overflows
    /// the `u16` step-address space is detectable at emit time instead of
    /// wrapping silently; `emit` rejects it before any address is used.
    total_steps: u32,
}

impl LayoutResult {
    /// Create a new layout result.
    pub fn new(label_to_step: BTreeMap<Label, StepAddr>, total_steps: u32) -> Self {
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
    pub fn total_steps(&self) -> u32 {
        self.total_steps
    }
}
