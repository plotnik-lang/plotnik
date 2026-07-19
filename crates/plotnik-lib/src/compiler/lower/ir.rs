//! Instruction IR with symbolic labels.
//!
//! Pre-layout instructions use `Label` for symbolic references.
//! After layout, labels are resolved to bytecode-word addresses (u16) for serialization.
//! Result-producing effects carry canonical member IDs assigned before
//! lowering, so every later consumer observes the same identity.

use std::collections::BTreeMap;

use crate::bytecode::{CodeAddr, EffectKind, EntryBoundary, Nav, PredicateOp, select_match_opcode};
use indexmap::IndexMap;

use crate::compiler::analyze::boundary::BoundaryState;
use crate::compiler::analyze::types::CaptureTypePlan;
use crate::compiler::ids::{DefId, ResultMemberId, TypeId};
use crate::compiler::lower::boundary::{ExitPort, ExitSignature};
use crate::compiler::lower::spans::SpanTable;
use crate::compiler::lower::thompson::boundary::{EntryObligation, NavigationContract};
use crate::core::NodeFieldId;

pub use plotnik_rt::PortId;

/// Node kind constraint for Match instructions.
///
/// The bytecode crate owns this type; re-exported here so IR producers and
/// consumers can name it as `ir::NodeKindConstraint`.
pub(crate) use crate::bytecode::NodeKindConstraint;

/// Symbolic reference, resolved to a bytecode-word address at layout time.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Label(pub u32);

impl Label {
    #[inline]
    pub fn resolve(self, map: &BTreeMap<Label, CodeAddr>) -> CodeAddr {
        *map.get(&self).expect("label not in layout")
    }
}

/// Label to continue at after a callee returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnAddr(pub Label);

/// Label where a callee definition starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalleeEntry(pub Label);

/// How a compiled definition body produces its pending value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DefOutputMode {
    Ordinary,
    Suppressed,
    CaptureType(CaptureTypePlan),
}

/// Copyable output provenance retained after a definition specialization is lowered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefOutputOrigin {
    Ordinary,
    Suppressed,
    CaptureType(TypeId),
}

impl DefOutputMode {
    pub(crate) fn origin(&self) -> DefOutputOrigin {
        match self {
            Self::Ordinary => DefOutputOrigin::Ordinary,
            Self::Suppressed => DefOutputOrigin::Suppressed,
            Self::CaptureType(plan) => DefOutputOrigin::CaptureType(plan.final_type()),
        }
    }
}

/// Whether explicit node-pattern matches contribute source provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SourceMode {
    Ordinary,
    Mark,
}

/// Return outcomes provided by a definition whose entry navigation is routed
/// through its body.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RoutedReturns {
    MatchOnly,
    Split,
}

/// Independent lowering choices for one definition body.
///
/// Keeping the axes in one semantic key prevents each new behavior from
/// growing its own parallel entry map. Equal call sites reuse the same body,
/// including through recursive components.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DefBodyMode {
    output: DefOutputMode,
    source: SourceMode,
}

impl DefBodyMode {
    pub(crate) fn ordinary() -> Self {
        Self {
            output: DefOutputMode::Ordinary,
            source: SourceMode::Ordinary,
        }
    }

    pub(crate) fn with_capture_type(mut self, plan: CaptureTypePlan) -> Self {
        self.output = DefOutputMode::CaptureType(plan);
        self
    }

    pub(crate) fn suppress_output(mut self) -> Self {
        self.output = DefOutputMode::Suppressed;
        self
    }

    pub(crate) fn mark_source(mut self) -> Self {
        self.source = SourceMode::Mark;
        self
    }

    pub(crate) fn output(&self) -> &DefOutputMode {
        &self.output
    }

    pub(crate) fn marks_source(&self) -> bool {
        self.source == SourceMode::Mark
    }

    pub(crate) fn source(&self) -> SourceMode {
        self.source
    }

    pub(crate) fn suppresses_output(&self) -> bool {
        matches!(self.output, DefOutputMode::Suppressed)
    }

    pub(crate) fn has_capture_type(&self) -> bool {
        matches!(self.output, DefOutputMode::CaptureType(_))
    }

    pub(crate) fn is_ordinary(&self) -> bool {
        matches!(self.output, DefOutputMode::Ordinary) && self.source == SourceMode::Ordinary
    }
}

/// Navigation and return routing for one definition body.
///
/// Ordinary calls navigate before entering an exact body. Recursive nullable
/// calls route navigation into the body instead, so the body's authored alternative
/// order remains above any candidate-search checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DefRoute {
    Caller,
    Routed { nav: Nav, returns: RoutedReturns },
}

impl DefRoute {
    fn routed(nav: Nav, returns: RoutedReturns) -> Self {
        assert!(
            nav != Nav::Epsilon,
            "definition entry must navigate or stay"
        );
        Self::Routed { nav, returns }
    }

    pub(crate) fn match_only(nav: Nav) -> Self {
        Self::routed(nav, RoutedReturns::MatchOnly)
    }

    pub(crate) fn split(nav: Nav) -> Self {
        Self::routed(nav, RoutedReturns::Split)
    }

    pub(crate) fn body_nav(self) -> Nav {
        match self {
            Self::Caller => Nav::StayExact,
            Self::Routed { nav, .. } => nav,
        }
    }

    pub(crate) fn splits(self) -> bool {
        matches!(
            self,
            Self::Routed {
                returns: RoutedReturns::Split,
                ..
            }
        )
    }

    pub(crate) fn requires_consumption(self) -> bool {
        matches!(
            self,
            Self::Routed {
                returns: RoutedReturns::MatchOnly,
                ..
            }
        )
    }

    pub(crate) fn return_depth(self, consumed: bool) -> Option<i32> {
        match (self, consumed) {
            (Self::Caller, true) => Some(0),
            (Self::Caller, false) => None,
            (Self::Routed { nav, .. }, true) => Some(nav.depth_delta()),
            (
                Self::Routed {
                    returns: RoutedReturns::Split,
                    ..
                },
                false,
            ) => Some(0),
            (
                Self::Routed {
                    returns: RoutedReturns::MatchOnly,
                    ..
                },
                false,
            ) => None,
        }
    }
}

/// One memoized definition body and its lowering mode.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DefSpecialization {
    def_id: DefId,
    mode: DefBodyMode,
    route: DefRoute,
    boundary: Option<DefBoundaryContract>,
    ports: ExitSignature,
}

/// Entry-side semantic contract owned by a boundary-aware specialization.
///
/// The exact reachable port signature remains on [`DefSpecialization`] because
/// ordinary and boundary-aware bodies share the same generalized return ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DefBoundaryContract {
    input: BoundaryState,
    entry: EntryObligation,
}

/// Immutable entry protocol implemented by one specialized definition body.
///
/// Caller-owned bodies are exact: navigation and field selection happen at
/// each call site. Callee-owned bodies embed one exact obligation into their
/// nullable structure, so every call targeting that body must agree with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CalleeEntryContract {
    CallerOwned,
    CalleeOwned { obligation: EntryObligation },
}

impl DefBoundaryContract {
    pub(crate) fn input(self) -> BoundaryState {
        self.input
    }

    pub(crate) fn entry(self) -> EntryObligation {
        self.entry
    }
}

impl DefSpecialization {
    pub(crate) fn ordinary(def_id: DefId) -> Self {
        Self {
            def_id,
            mode: DefBodyMode::ordinary(),
            route: DefRoute::Caller,
            boundary: None,
            ports: ExitSignature::singleton(ExitPort::ConsumedOtherNone),
        }
    }

    pub(crate) fn new(def_id: DefId, mode: DefBodyMode) -> Self {
        Self {
            def_id,
            mode,
            route: DefRoute::Caller,
            boundary: None,
            ports: ExitSignature::singleton(ExitPort::ConsumedOtherNone),
        }
    }

    pub(crate) fn routed_match(def_id: DefId, mode: DefBodyMode, nav: Nav) -> Self {
        Self {
            def_id,
            mode,
            route: DefRoute::match_only(nav),
            boundary: None,
            ports: ExitSignature::singleton(ExitPort::ConsumedOtherNone),
        }
    }

    pub(crate) fn routed_split(def_id: DefId, mode: DefBodyMode, nav: Nav) -> Self {
        Self {
            def_id,
            mode,
            route: DefRoute::split(nav),
            boundary: None,
            ports: ExitSignature::from_ports([ExitPort::ConsumedOtherNone, ExitPort::EmptyNone]),
        }
    }

    pub(crate) fn boundary(
        def_id: DefId,
        mode: DefBodyMode,
        input: BoundaryState,
        entry: EntryObligation,
        ports: ExitSignature,
    ) -> Self {
        let route = if ports.ports().iter().any(|port| !port.consumed()) {
            DefRoute::split(entry.navigation().authored())
        } else {
            DefRoute::match_only(entry.navigation().authored())
        };
        Self {
            def_id,
            mode,
            route,
            boundary: Some(DefBoundaryContract { input, entry }),
            ports,
        }
    }

    pub(crate) fn def_id(&self) -> DefId {
        self.def_id
    }

    pub(crate) fn mode(&self) -> &DefBodyMode {
        &self.mode
    }

    pub(crate) fn route(&self) -> DefRoute {
        self.route
    }

    pub(crate) fn boundary_contract(&self) -> Option<DefBoundaryContract> {
        self.boundary
    }

    pub(crate) fn ports(&self) -> &ExitSignature {
        &self.ports
    }

    pub(crate) fn is_ordinary(&self) -> bool {
        self.mode.is_ordinary()
            && self.route == DefRoute::Caller
            && self.boundary.is_none()
            && self.ports.ports() == [ExitPort::ConsumedOtherNone]
    }
}

/// Effect operation whose structured-result arguments use canonical member IDs.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectIR {
    kind: EffectKind,
    payload: EffectArg,
}

/// An effect's argument: a literal value, or a canonical field/case identity.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EffectArg {
    Literal(usize),
    Member(ResultMemberId),
}

impl EffectIR {
    /// The effect's kind.
    #[inline]
    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    pub(crate) fn argument(&self) -> &EffectArg {
        &self.payload
    }

    /// Create a literal effect without member reference.
    pub fn literal(kind: EffectKind, payload: usize) -> Self {
        Self {
            kind,
            payload: EffectArg::Literal(payload),
        }
    }

    pub fn with_member(kind: EffectKind, member: ResultMemberId) -> Self {
        Self {
            kind,
            payload: EffectArg::Member(member),
        }
    }

    /// Capture current node value.
    pub fn node() -> Self {
        Self::literal(EffectKind::Node, 0)
    }

    /// Produce an absent value.
    pub fn absent() -> Self {
        Self::literal(EffectKind::Absent, 0)
    }

    /// Append the pending value to the open list.
    pub fn array_push() -> Self {
        Self::literal(EffectKind::ArrayPush, 0)
    }

    /// Begin a list value.
    pub fn list_open() -> Self {
        Self::literal(EffectKind::ListOpen, 0)
    }

    /// End a list value.
    pub fn list_close() -> Self {
        Self::literal(EffectKind::ListClose, 0)
    }

    /// Begin a record value.
    pub fn record_open() -> Self {
        Self::literal(EffectKind::RecordOpen, 0)
    }

    /// End a record value.
    pub fn record_close() -> Self {
        Self::literal(EffectKind::RecordClose, 0)
    }

    /// End variant scope.
    pub fn end_variant() -> Self {
        Self::literal(EffectKind::VariantClose, 0)
    }

    /// Begin suppression (suppress effects within).
    pub fn suppress_begin() -> Self {
        Self::literal(EffectKind::SuppressBegin, 0)
    }

    /// End suppression.
    pub fn suppress_end() -> Self {
        Self::literal(EffectKind::SuppressEnd, 0)
    }

    pub fn scalar_open() -> Self {
        Self::literal(EffectKind::ScalarOpen, 0)
    }

    pub fn scalar_mark() -> Self {
        Self::literal(EffectKind::ScalarMark, 0)
    }

    pub fn text_close() -> Self {
        Self::literal(EffectKind::TextClose, 0)
    }

    pub fn bool_close(value: bool) -> Self {
        Self::literal(EffectKind::BoolClose, usize::from(value))
    }

    pub fn node_text() -> Self {
        Self::literal(EffectKind::NodeText, 0)
    }

    pub fn node_bool() -> Self {
        Self::literal(EffectKind::NodeBool, 0)
    }

    pub fn bool_value(value: bool) -> Self {
        Self::literal(EffectKind::BoolValue, usize::from(value))
    }

    /// Open an inspection span and snapshot the current cursor node.
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
            Self::Call(call) => {
                if call.returns.len() == 1 {
                    8
                } else {
                    24
                }
            }
            Self::Return(_) => 8,
        }
    }

    /// Get all successor labels (for graph building).
    pub fn successors(&self) -> &[Label] {
        match self {
            Self::Match(m) => &m.successors,
            Self::Call(c) => c.return_labels(),
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
    /// Node must be a tree-sitter MISSING node — the `(MISSING …)` constraint.
    pub missing: bool,
    /// Effects to execute after a successful match, in bytecode order.
    pub effects: Vec<EffectIR>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<NodeFieldId>,
    /// Predicate for node text filtering (None = no text check).
    pub predicate: Option<PredicateIR>,
    /// Successor labels (empty = accept, 1 = linear, 2+ = fork).
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
            missing: false,
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

    pub fn missing(mut self, missing: bool) -> Self {
        self.missing = missing;
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
    /// Which side of the call boundary discharges entry navigation and field
    /// selection.
    pub entry: CallEntry,
    /// Callee entry point.
    pub target: Label,
    /// Continuations indexed by the callee-local dense [`PortId`].
    pub returns: Vec<Label>,
}

/// Entry ownership for a definition call.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallEntry {
    /// The call instruction navigates and checks the field before entering an
    /// exact callee body.
    CallerOwned {
        nav: Nav,
        field: Option<NodeFieldId>,
    },
    /// The specialized callee carries the entry obligation through nullable
    /// structure and discharges it at each eventual first consumer.
    CalleeOwned { obligation: EntryObligation },
}

impl CallEntry {
    pub fn nav(self) -> Nav {
        match self {
            Self::CallerOwned { nav, .. } => nav,
            Self::CalleeOwned { obligation } => obligation.navigation().authored(),
        }
    }

    pub fn field(self) -> Option<NodeFieldId> {
        match self {
            Self::CallerOwned { field, .. } => field,
            Self::CalleeOwned { obligation } => obligation.field(),
        }
    }

    pub fn caller_owned(self) -> bool {
        matches!(self, Self::CallerOwned { .. })
    }

    pub fn continuation_depth(self, consumed: bool) -> Option<i32> {
        match (self, consumed) {
            (Self::CallerOwned { nav, .. }, true) => Some(nav.depth_delta()),
            (Self::CallerOwned { .. }, false) => None,
            (Self::CalleeOwned { obligation }, true) => {
                Some(obligation.navigation().authored().depth_delta())
            }
            (Self::CalleeOwned { .. }, false) => Some(0),
        }
    }
}

impl CallIR {
    /// Create a call instruction with default nav (Stay) and no field constraint.
    pub fn new(label: Label, return_addr: ReturnAddr, callee: CalleeEntry) -> Self {
        Self {
            label,
            entry: CallEntry::CallerOwned {
                nav: Nav::Stay,
                field: None,
            },
            target: callee.0,
            returns: vec![return_addr.0],
        }
    }

    /// Create a matched-only call whose callee owns entry navigation.
    pub fn routed(
        label: Label,
        entry_nav: Nav,
        return_addr: ReturnAddr,
        callee: CalleeEntry,
    ) -> Self {
        Self {
            label,
            entry: CallEntry::CalleeOwned {
                obligation: EntryObligation::new(NavigationContract::from_nav(entry_nav)),
            },
            target: callee.0,
            returns: vec![return_addr.0],
        }
    }

    /// Create a call whose nullable callee reports node-consuming and empty
    /// outcomes separately. Navigation belongs to the routed callee variant.
    pub fn split(
        label: Label,
        entry_nav: Nav,
        returns: SplitReturnAddrs,
        callee: CalleeEntry,
    ) -> Self {
        Self {
            label,
            entry: CallEntry::CalleeOwned {
                obligation: EntryObligation::new(NavigationContract::from_nav(entry_nav)),
            },
            target: callee.0,
            returns: vec![returns.matched.0, returns.empty.0],
        }
    }

    pub fn generalized(
        label: Label,
        entry: CallEntry,
        returns: Vec<Label>,
        callee: CalleeEntry,
    ) -> Self {
        assert!(
            !returns.is_empty() && returns.len() <= usize::from(PortId::COUNT),
            "call must provide 1..={} continuations",
            PortId::COUNT
        );
        Self {
            label,
            entry,
            target: callee.0,
            returns,
        }
    }

    pub fn nav(mut self, nav: Nav) -> Self {
        let CallEntry::CallerOwned {
            nav: current_nav, ..
        } = &mut self.entry
        else {
            panic!("routed calls derive navigation from their callee route")
        };
        *current_nav = nav;
        self
    }

    pub fn node_field(mut self, f: impl Into<Option<NodeFieldId>>) -> Self {
        let field = f.into();
        match &mut self.entry {
            CallEntry::CallerOwned { field: current, .. } => {
                *current = field;
            }
            CallEntry::CalleeOwned { obligation } => {
                assert!(
                    obligation.field().is_none(),
                    "a callee-owned call must discharge at most one grammar field"
                );
                if let Some(field) = field {
                    *obligation = obligation.with_field(field);
                }
            }
        }
        self
    }

    pub fn field(&self) -> Option<NodeFieldId> {
        self.entry.field()
    }

    pub fn return_labels(&self) -> &[Label] {
        &self.returns
    }

    pub(crate) fn remap_returns(&mut self, mut resolve: impl FnMut(Label) -> Label) {
        for target in &mut self.returns {
            *target = resolve(*target);
        }
    }
}

/// The two semantic continuations of a nullable recursive call.
///
/// Bundling the same-typed labels prevents callers from silently transposing
/// the node-consuming and empty routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SplitReturnAddrs {
    pub matched: ReturnAddr,
    pub empty: ReturnAddr,
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
    /// Callee-local dense exit port.
    pub port: PortId,
    /// Entry protocol implemented by the body containing this return.
    pub(crate) entry: CalleeEntryContract,
}

impl ReturnIR {
    pub fn new(label: Label) -> Self {
        Self::port(label, PortId::ZERO)
    }

    pub fn port(label: Label, port: PortId) -> Self {
        Self {
            label,
            port,
            entry: CalleeEntryContract::CallerOwned,
        }
    }

    pub(crate) fn callee_owned(label: Label, port: PortId, obligation: EntryObligation) -> Self {
        Self {
            label,
            port,
            entry: CalleeEntryContract::CalleeOwned { obligation },
        }
    }

    pub fn matched(label: Label) -> Self {
        Self::new(label)
    }

    pub fn routed_matched(label: Label, obligation: EntryObligation) -> Self {
        Self::callee_owned(label, PortId::ZERO, obligation)
    }

    pub fn routed_empty(label: Label, obligation: EntryObligation) -> Self {
        Self::callee_owned(
            label,
            PortId::from_byte(1).expect("one is a valid port id"),
            obligation,
        )
    }
}

impl From<ReturnIR> for InstructionIR {
    fn from(r: ReturnIR) -> Self {
        Self::Return(r)
    }
}

/// Which compilation window a label was allocated in — per-instruction
/// provenance for dumps and generated-code comments.
///
/// Attribution is by *physical* location: labels created while a nullable ref
/// is inlined into a host definition belong to the host (the instruction lives
/// in the host's body). Post-build passes only delete and rewire instructions,
/// never renumber labels, so an origin recorded at allocation stays valid for
/// every surviving label. Labels minted later by `pack_instructions` have no
/// origin — they are wire-format artifacts with no source correspondence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabelOrigin {
    /// Allocated while compiling this definition's body.
    Def(DefId),
    /// Allocated while compiling a non-ordinary definition specialization.
    DefSpecialization {
        def_id: DefId,
        output: DefOutputOrigin,
        source: SourceMode,
        route: DefRoute,
    },
}

/// One selectable definition and the result effects owned by its execution boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntryPointIR {
    pub(crate) target: Label,
    pub(crate) boundary: EntryBoundary,
}

/// Compiled query IR plus entry labels produced by the compile stage.
#[derive(Clone, Debug)]
pub struct NfaGraph {
    pub(in crate::compiler::lower) instructions: Vec<InstructionIR>,
    /// Entry labels for every emitted definition specialization.
    pub(in crate::compiler::lower) def_entries: IndexMap<DefSpecialization, Label>,
    /// Selectable definitions in `DefId` order.
    pub(in crate::compiler::lower) entry_points: IndexMap<DefId, EntryPointIR>,
    /// Inspection span table, present iff the query was compiled with inspection.
    pub(in crate::compiler::lower) spans: Option<SpanTable>,
    /// Origin per label id (index = `Label.0`), recorded at allocation.
    pub(in crate::compiler::lower) label_origins: Vec<Option<LabelOrigin>>,
}

impl NfaGraph {
    pub(crate) fn instructions(&self) -> &[InstructionIR] {
        &self.instructions
    }

    pub(crate) fn entry_points(&self) -> &IndexMap<DefId, EntryPointIR> {
        &self.entry_points
    }

    pub(crate) fn specialization_for_entry(&self, entry: Label) -> Option<&DefSpecialization> {
        self.def_entries
            .iter()
            .find_map(|(specialization, &label)| (label == entry).then_some(specialization))
    }

    pub(crate) fn spans(&self) -> Option<&SpanTable> {
        self.spans.as_ref()
    }

    /// The compilation window `label` was allocated in, or `None` for labels
    /// minted by post-build passes (pack cascades).
    pub(crate) fn origin(&self, label: Label) -> Option<LabelOrigin> {
        self.label_origins.get(label.0 as usize).copied().flatten()
    }
}

/// The optimized NFA before wire packing — the last pipeline artifact every
/// backend shares.
///
/// This is the fork point between executors: the bytecode path packs it for
/// the wire (`pack_instructions` splits instructions that exceed wire slot
/// limits into epsilon cascades); code generation consumes it directly, with
/// symbolic labels and provenance intact. Everything semantic — anchors,
/// nullable inlining, null-defaulting, greediness, dedup — has already
/// happened by this point.
#[derive(Clone, Debug)]
pub struct SemanticNfa {
    raw: NfaGraph,
}

impl SemanticNfa {
    pub(super) fn new(raw: NfaGraph) -> Self {
        Self { raw }
    }

    pub(crate) fn raw(&self) -> &NfaGraph {
        &self.raw
    }

    pub(super) fn into_raw(self) -> NfaGraph {
        self.raw
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
