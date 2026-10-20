use std::collections::BTreeMap;

use crate::bytecode::{CodeAddr, EffectKind, EntryBoundary, MatchCounts, Nav, PredicateOp};
use indexmap::IndexMap;

use crate::compiler::analyze::boundary::BoundaryState;
use crate::compiler::analyze::boundary::ExitPort;
use crate::compiler::analyze::types::CaptureTypePlan;
use crate::compiler::ids::{DefId, ResultMemberId, TypeId};
use crate::compiler::lower::boundary::ExitSignature;
use crate::compiler::lower::spans::SpanTable;
use crate::compiler::lower::thompson::boundary::{EntryObligation, NavigationContract};
use crate::core::{NodeFieldId, NodeKindId};

pub use plotnik_runtime::PortId;

pub(crate) use crate::bytecode::NodeKindConstraint;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Label(pub u32);

impl Label {
    #[inline]
    pub fn resolve(self, map: &BTreeMap<Label, CodeAddr>) -> CodeAddr {
        *map.get(&self).expect("label not in layout")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReturnAddr(pub Label);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalleeEntry(pub Label);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DefOutputMode {
    Ordinary,
    Suppressed,
    CaptureType(CaptureTypePlan),
}

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum SourceMode {
    Ordinary,
    Mark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RoutedReturns {
    MatchOnly,
    Split,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DefSpecialization {
    def_id: DefId,
    mode: DefBodyMode,
    route: DefRoute,
    boundary: Option<DefBoundaryContract>,
    ports: ExitSignature,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DefBoundaryContract {
    input: BoundaryState,
    entry: EntryObligation,
}

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct EffectIR {
    kind: EffectKind,
    payload: EffectArg,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum EffectArg {
    Literal(usize),
    Member(ResultMemberId),
}

impl EffectIR {
    #[inline]
    pub fn kind(&self) -> EffectKind {
        self.kind
    }

    pub(crate) fn argument(&self) -> &EffectArg {
        &self.payload
    }

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

    pub fn node() -> Self {
        Self::literal(EffectKind::Node, 0)
    }

    pub fn absent() -> Self {
        Self::literal(EffectKind::Absent, 0)
    }

    pub fn array_push() -> Self {
        Self::literal(EffectKind::ArrayPush, 0)
    }

    pub fn list_open() -> Self {
        Self::literal(EffectKind::ListOpen, 0)
    }

    pub fn list_close() -> Self {
        Self::literal(EffectKind::ListClose, 0)
    }

    pub fn record_open() -> Self {
        Self::literal(EffectKind::RecordOpen, 0)
    }

    pub fn record_close() -> Self {
        Self::literal(EffectKind::RecordClose, 0)
    }

    pub fn end_variant() -> Self {
        Self::literal(EffectKind::VariantClose, 0)
    }

    pub fn suppress_begin() -> Self {
        Self::literal(EffectKind::SuppressBegin, 0)
    }

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

    pub fn span_start_at(id: u16) -> Self {
        Self::literal(EffectKind::SpanStartAt, id as usize)
    }

    pub fn span_start(id: u16) -> Self {
        Self::literal(EffectKind::SpanStart, id as usize)
    }

    pub fn span_end(id: u16) -> Self {
        Self::literal(EffectKind::SpanEnd, id as usize)
    }

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PredicateValueIR {
    String(Box<str>),
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
}

#[derive(Clone, Debug)]
pub enum InstructionIR {
    Match(MatchIR),
    Switch(SwitchIR),
    Call(CallIR),
    Return(ReturnIR),
}

impl InstructionIR {
    #[inline]
    pub fn label(&self) -> Label {
        match self {
            Self::Match(m) => m.label,
            Self::Switch(s) => s.label,
            Self::Call(c) => c.label,
            Self::Return(r) => r.label,
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Self::Match(m) => m.size(),
            Self::Switch(_) => 8,
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

    pub fn successors(&self) -> &[Label] {
        match self {
            Self::Match(m) => &m.successors,
            Self::Switch(s) => &s.targets,
            Self::Call(c) => c.return_labels(),
            Self::Return(_) => &[],
        }
    }
}

#[derive(Clone, Debug)]
pub struct SwitchIR {
    pub label: Label,
    kinds: Vec<NodeKindId>,
    targets: Vec<Label>,
}

impl SwitchIR {
    pub fn new(label: Label, cases: Vec<(NodeKindId, Label)>) -> Self {
        let (kinds, targets) = cases.into_iter().unzip();
        Self {
            label,
            kinds,
            targets,
        }
    }

    pub fn cases(&self) -> impl ExactSizeIterator<Item = (NodeKindId, Label)> + '_ {
        self.kinds.iter().copied().zip(self.targets.iter().copied())
    }

    pub fn remap_targets(&mut self, resolve: impl Fn(Label) -> Label) {
        for target in &mut self.targets {
            *target = resolve(*target);
        }
    }
}

#[derive(Clone, Debug)]
pub struct MatchIR {
    pub label: Label,
    pub nav: Nav,
    pub node_kind: NodeKindConstraint,
    pub node_field: Option<NodeFieldId>,
    pub missing: bool,
    pub effects: Vec<EffectIR>,
    pub neg_fields: Vec<NodeFieldId>,
    pub predicate: Option<PredicateIR>,
    pub successors: Vec<Label>,
}

impl MatchIR {
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
        MatchCounts::new(
            self.effects.len(),
            self.neg_fields.len(),
            self.successors.len(),
            self.predicate.as_ref().map(|predicate| predicate.op),
            self.missing,
        )
        .and_then(MatchCounts::opcode)
        .expect("instruction fits a match opcode")
        .size()
    }

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

#[derive(Clone, Debug)]
pub struct CallIR {
    pub label: Label,
    pub entry: CallEntry,
    pub target: Label,
    pub returns: Vec<Label>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CallEntry {
    CallerOwned {
        nav: Nav,
        field: Option<NodeFieldId>,
    },
    CalleeOwned {
        obligation: EntryObligation,
    },
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

#[derive(Clone, Debug)]
pub struct ReturnIR {
    pub label: Label,
    pub port: PortId,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LabelOrigin {
    Expression,
    Def(DefId),
    DefSpecialization {
        def_id: DefId,
        output: DefOutputOrigin,
        source: SourceMode,
        route: DefRoute,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum EntrypointKey {
    Named(DefId),
    Expression,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct EntrypointIR {
    pub(crate) target: Label,
    pub(crate) boundary: EntryBoundary,
}

#[derive(Clone, Debug)]
pub struct NfaGraph {
    pub(in crate::compiler::lower) instructions: Vec<InstructionIR>,
    pub(in crate::compiler::lower) def_entries: IndexMap<DefSpecialization, Label>,
    pub(in crate::compiler::lower) entrypoints: IndexMap<EntrypointKey, EntrypointIR>,
    pub(in crate::compiler::lower) spans: Option<SpanTable>,
    pub(in crate::compiler::lower) label_origins: Vec<Option<LabelOrigin>>,
}

impl NfaGraph {
    pub(crate) fn instructions(&self) -> &[InstructionIR] {
        &self.instructions
    }

    pub(crate) fn entrypoints(&self) -> &IndexMap<EntrypointKey, EntrypointIR> {
        &self.entrypoints
    }

    pub(crate) fn executable_entries(&self) -> impl Iterator<Item = &EntrypointIR> {
        self.entrypoints.values()
    }

    pub(crate) fn specialization_for_entry(&self, entry: Label) -> Option<&DefSpecialization> {
        self.def_entries
            .iter()
            .find_map(|(specialization, &label)| (label == entry).then_some(specialization))
    }

    pub(crate) fn spans(&self) -> Option<&SpanTable> {
        self.spans.as_ref()
    }

    pub(crate) fn origin(&self, label: Label) -> Option<LabelOrigin> {
        self.label_origins.get(label.0 as usize).copied().flatten()
    }
}

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
