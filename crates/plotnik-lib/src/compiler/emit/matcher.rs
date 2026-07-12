//! Target-neutral matcher plan.
//!
//! This is the semantic half of generated matcher emission. It assigns dense
//! runtime state ids, preserves dump provenance, orders candidate checks,
//! records exact search and retry policies, resolves capture-member effects,
//! and resolves symbolic successor labels. Backends render this data without
//! inspecting the NFA again.

use std::collections::BTreeMap;

use plotnik_rt::{Nav, SkipPolicy};
use regex_syntax::hir::Hir;

use crate::bytecode::{EffectKind, NodeKindConstraint, PredicateOp};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::output::CaptureLayout;
use crate::compiler::ids::DefId;
use crate::compiler::lower::dump::NfaDumper;
use crate::compiler::lower::ir::{
    CallIR, CallProtocol, EffectArg, EffectIR, InstructionIR, Label, LabelOrigin, MatchIR,
    NfaGraph, PredicateIR, PredicateValueIR,
};
use crate::compiler::regex::normalize;
use crate::core::{NodeFieldId, NodeKindId};

/// Dense runtime state id carried by frames and checkpoints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StateId(u16);

impl StateId {
    pub(crate) fn raw(self) -> u16 {
        self.0
    }
}

/// Regex table id, assigned in first-appearance order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegexId(usize);

impl RegexId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StateOrigin {
    Definition,
    ConsumingDefinition,
    Entrypoint,
}

#[derive(Clone, Debug)]
pub(crate) struct StatePlan {
    pub(crate) id: StateId,
    pub(crate) label: Label,
    pub(crate) definition: String,
    pub(crate) origin: StateOrigin,
    /// The instruction in the canonical NFA dump format.
    pub(crate) provenance: String,
    pub(crate) kind: StatePlanKind,
}

#[derive(Clone, Debug)]
pub(crate) enum StatePlanKind {
    Epsilon {
        effects: Vec<EffectPlan>,
        flow: FlowPlan,
    },
    Match(MatchPlan),
    Call(CallPlan),
    Return(plotnik_rt::ReturnOutcome),
}

#[derive(Clone, Debug)]
pub(crate) struct MatchPlan {
    pub(crate) nav: Nav,
    /// Policy used while scanning for the first acceptable candidate.
    pub(crate) search: SkipPolicy,
    /// Policy stored in a match-retry checkpoint, when this state owns one.
    pub(crate) retry: Option<SkipPolicy>,
    pub(crate) checks: Vec<CheckPlan>,
    pub(crate) effects: Vec<EffectPlan>,
    pub(crate) flow: FlowPlan,
    pub(crate) candidate_pattern: String,
}

impl MatchPlan {
    pub(crate) fn navigates(&self) -> bool {
        !matches!(self.nav, Nav::Stay | Nav::StayExact)
    }

    pub(crate) fn has_candidate_checks(&self) -> bool {
        !self.checks.is_empty()
    }

    pub(crate) fn has_predicate(&self) -> bool {
        self.checks
            .iter()
            .any(|check| matches!(check, CheckPlan::Predicate(_)))
    }

    pub(crate) fn needs_node_binding(&self) -> bool {
        self.checks.iter().any(CheckPlan::reads_node)
    }

    /// Whether candidate setup can fail before this state's terminal flow.
    pub(crate) fn can_fail_before_flow(&self) -> bool {
        self.navigates() || self.has_candidate_checks()
    }
}

#[derive(Clone, Debug)]
pub(crate) enum CallPlan {
    Ordinary(OrdinaryCallPlan),
    Routed(RoutedCallPlan),
    Split(SplitCallPlan),
}

#[derive(Clone, Debug)]
pub(crate) struct OrdinaryCallPlan {
    pub(crate) nav: Nav,
    pub(crate) search: SkipPolicy,
    pub(crate) retry: Option<SkipPolicy>,
    pub(crate) field: Option<u16>,
    pub(crate) target: StateId,
    pub(crate) next: StateId,
}

impl OrdinaryCallPlan {
    pub(crate) fn stays_on_current_node(&self) -> bool {
        matches!(self.nav, Nav::Stay | Nav::StayExact)
    }

    /// Whether navigation or field selection can fail before entering the call.
    pub(crate) fn can_fail_before_flow(&self) -> bool {
        !self.stays_on_current_node() || self.field.is_some()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SplitCallPlan {
    pub(crate) target: StateId,
    pub(crate) matched: StateId,
    pub(crate) zero: StateId,
}

#[derive(Clone, Debug)]
pub(crate) struct RoutedCallPlan {
    pub(crate) target: StateId,
    pub(crate) next: StateId,
}

#[derive(Clone, Debug)]
pub(crate) enum FlowPlan {
    Accept,
    Jump(StateId),
    Branch {
        next: StateId,
        /// Checkpoints in the NFA's declared order. The runtime pushes this
        /// slice with its established reverse/LIFO discipline.
        alternatives: Vec<StateId>,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct EffectPlan {
    pub(crate) kind: EffectKind,
    /// Absolute capture-member slot for `Set`/`EnumOpen`; literal payload for
    /// the other effect kinds.
    pub(crate) payload: u16,
    pub(crate) display: String,
}

#[derive(Clone, Debug)]
pub(crate) enum CheckPlan {
    Kind(KindCheck),
    Missing,
    Field(FieldPlan),
    NegField(FieldPlan),
    Predicate(PredicatePlan),
}

impl CheckPlan {
    fn reads_node(&self) -> bool {
        !matches!(self, Self::Field(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KindClass {
    Named,
    Anonymous,
}

#[derive(Clone, Debug)]
pub(crate) struct KindCheck {
    pub(crate) class: KindClass,
    pub(crate) id: Option<u16>,
    pub(crate) name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldPlan {
    pub(crate) id: u16,
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PredicatePlan {
    pub(crate) op: PredicateOp,
    pub(crate) value: PredicateValuePlan,
}

#[derive(Clone, Debug)]
pub(crate) enum PredicateValuePlan {
    String(String),
    Regex { id: RegexId, pattern: String },
}

#[derive(Clone, Debug)]
pub(crate) struct ExpectedKind {
    pub(crate) id: u16,
    pub(crate) name: String,
    pub(crate) named: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ExpectedField {
    pub(crate) id: u16,
    pub(crate) name: String,
}

#[derive(Clone, Debug)]
pub(crate) struct RegexPlan {
    pub(crate) id: RegexId,
    pub(crate) pattern: String,
    pub(crate) normalized: Hir,
}

#[derive(Clone, Debug)]
pub(crate) struct EntryPlan {
    pub(crate) definition: DefId,
    pub(crate) name: String,
    pub(crate) entry: StateId,
}

#[derive(Clone, Debug)]
pub(crate) struct MatcherPlan {
    states: Vec<StatePlan>,
    entrypoints: Vec<EntryPlan>,
    expected_kinds: Vec<ExpectedKind>,
    expected_fields: Vec<ExpectedField>,
    /// Fields in first-appearance order. Target representation passes use
    /// this order when resolving identifier collisions.
    fields: Vec<FieldPlan>,
    regexes: Vec<RegexPlan>,
    label_width: usize,
    any_predicate: bool,
    any_retry_predicate: bool,
}

impl MatcherPlan {
    pub(super) fn build(
        graph: &NfaGraph,
        artifacts: AnalysisArtifacts<'_>,
        layout: &CaptureLayout,
    ) -> Self {
        let dumper = NfaDumper::new(graph, artifacts);
        let mut sorted: Vec<&InstructionIR> = graph.instructions().iter().collect();
        sorted.sort_by_key(|instruction| instruction.label());
        assert!(
            sorted.len() <= u16::MAX as usize + 1,
            "state space exceeds u16 ids"
        );

        let ids = sorted
            .iter()
            .enumerate()
            .map(|(index, instruction)| {
                let raw = u16::try_from(index).expect("validated state count fits u16 ids");
                (instruction.label(), StateId(raw))
            })
            .collect::<BTreeMap<_, _>>();
        let entrypoints = graph
            .entrypoint_wrappers()
            .iter()
            .map(|(&definition, &label)| {
                let symbol = artifacts.dependency_analysis.def_name_sym(definition);
                EntryPlan {
                    definition,
                    name: artifacts.interner.resolve(symbol).to_string(),
                    entry: resolve_state(&ids, label),
                }
            })
            .collect();

        let mut builder = MatcherPlanBuilder::new(&dumper, artifacts, layout, &ids);
        let states = sorted
            .into_iter()
            .enumerate()
            .map(|(index, instruction)| builder.state(graph, index, instruction))
            .collect();

        Self {
            states,
            entrypoints,
            expected_kinds: builder.expected_kinds.into_values().collect(),
            expected_fields: builder.expected_fields.into_values().collect(),
            fields: builder.fields,
            regexes: builder.regexes,
            label_width: dumper.label_width(),
            any_predicate: builder.any_predicate,
            any_retry_predicate: builder.any_retry_predicate,
        }
    }

    pub(crate) fn states(&self) -> &[StatePlan] {
        &self.states
    }

    pub(crate) fn entrypoints(&self) -> &[EntryPlan] {
        &self.entrypoints
    }

    pub(crate) fn expected_kinds(&self) -> &[ExpectedKind] {
        &self.expected_kinds
    }

    pub(crate) fn expected_fields(&self) -> &[ExpectedField] {
        &self.expected_fields
    }

    pub(crate) fn fields(&self) -> &[FieldPlan] {
        &self.fields
    }

    pub(crate) fn regexes(&self) -> &[RegexPlan] {
        &self.regexes
    }

    pub(crate) fn label_width(&self) -> usize {
        self.label_width
    }

    pub(crate) fn any_predicate(&self) -> bool {
        self.any_predicate
    }

    pub(crate) fn any_retry_predicate(&self) -> bool {
        self.any_retry_predicate
    }
}

struct MatcherPlanBuilder<'p, 'a> {
    dumper: &'p NfaDumper<'a>,
    artifacts: AnalysisArtifacts<'a>,
    layout: &'p CaptureLayout,
    ids: &'p BTreeMap<Label, StateId>,
    expected_kinds: BTreeMap<u16, ExpectedKind>,
    expected_fields: BTreeMap<u16, ExpectedField>,
    fields: Vec<FieldPlan>,
    regex_ids: BTreeMap<String, RegexId>,
    regexes: Vec<RegexPlan>,
    any_predicate: bool,
    any_retry_predicate: bool,
}

impl<'p, 'a> MatcherPlanBuilder<'p, 'a> {
    fn new(
        dumper: &'p NfaDumper<'a>,
        artifacts: AnalysisArtifacts<'a>,
        layout: &'p CaptureLayout,
        ids: &'p BTreeMap<Label, StateId>,
    ) -> Self {
        Self {
            dumper,
            artifacts,
            layout,
            ids,
            expected_kinds: BTreeMap::new(),
            expected_fields: BTreeMap::new(),
            fields: Vec::new(),
            regex_ids: BTreeMap::new(),
            regexes: Vec::new(),
            any_predicate: false,
            any_retry_predicate: false,
        }
    }

    fn state(&mut self, graph: &NfaGraph, index: usize, instruction: &InstructionIR) -> StatePlan {
        let label = instruction.label();
        let origin = match graph
            .origin(label)
            .expect("every pre-pack label carries an origin")
        {
            LabelOrigin::Def(_) => StateOrigin::Definition,
            LabelOrigin::DefVariant { route, .. } => {
                if route.requires_consumption() {
                    StateOrigin::ConsumingDefinition
                } else {
                    StateOrigin::Definition
                }
            }
            LabelOrigin::Wrapper(_) => StateOrigin::Entrypoint,
        };
        let kind = match instruction {
            InstructionIR::Match(instruction) => self.match_state(instruction),
            InstructionIR::Call(instruction) => StatePlanKind::Call(self.call_state(instruction)),
            InstructionIR::Return(return_) => StatePlanKind::Return(return_.outcome()),
        };
        StatePlan {
            id: StateId(u16::try_from(index).expect("validated state count fits u16 ids")),
            label,
            definition: self.dumper.def_name_of(label).to_string(),
            origin,
            provenance: self.dumper.render_instruction(instruction),
            kind,
        }
    }

    fn match_state(&mut self, instruction: &MatchIR) -> StatePlanKind {
        let effects = self.effects(&instruction.effects);
        let flow = self.flow(&instruction.successors);
        if instruction.is_epsilon() {
            assert!(
                matches!(instruction.node_kind, NodeKindConstraint::Any)
                    && !instruction.missing
                    && instruction.node_field.is_none()
                    && instruction.neg_fields.is_empty()
                    && instruction.predicate.is_none(),
                "epsilon match carries no candidate checks"
            );
            return StatePlanKind::Epsilon { effects, flow };
        }

        let search = instruction.nav.skip_policy();
        let retry = instruction.nav.is_sibling_search().then_some(search);
        let checks = self.checks(instruction, retry.is_some());
        StatePlanKind::Match(MatchPlan {
            nav: instruction.nav,
            search,
            retry,
            checks,
            effects,
            flow,
            candidate_pattern: self.dumper.node_pattern_display(instruction),
        })
    }

    fn call_state(&mut self, instruction: &CallIR) -> CallPlan {
        let target = resolve_state(self.ids, instruction.target);
        let matched = resolve_state(self.ids, instruction.matched_return());
        match instruction.protocol {
            CallProtocol::Split { returns, .. } => CallPlan::Split(SplitCallPlan {
                target,
                matched,
                zero: resolve_state(self.ids, returns[1]),
            }),
            CallProtocol::Routed { .. } => CallPlan::Routed(RoutedCallPlan {
                target,
                next: matched,
            }),
            CallProtocol::Ordinary {
                nav, node_field, ..
            } => {
                let field = node_field.map(|field| self.record_field(field).id);
                let stays = matches!(nav, Nav::Stay | Nav::StayExact);
                let search = nav.skip_policy();
                let retry = (!stays && search != SkipPolicy::Exact).then_some(search);
                CallPlan::Ordinary(OrdinaryCallPlan {
                    nav,
                    search,
                    retry,
                    field,
                    target,
                    next: matched,
                })
            }
        }
    }

    /// Candidate checks in the VM's normative order: kind, missing, field,
    /// negated fields, then text predicate.
    fn checks(&mut self, instruction: &MatchIR, retryable: bool) -> Vec<CheckPlan> {
        let mut checks = Vec::new();
        if let Some(kind) = self.kind_check(instruction.node_kind) {
            checks.push(CheckPlan::Kind(kind));
        }
        if instruction.missing {
            checks.push(CheckPlan::Missing);
        }
        if let Some(field) = instruction.node_field {
            checks.push(CheckPlan::Field(self.record_field(field)));
        }
        for &field in &instruction.neg_fields {
            checks.push(CheckPlan::NegField(self.record_field(field)));
        }
        if let Some(predicate) = &instruction.predicate {
            self.any_predicate = true;
            self.any_retry_predicate |= retryable;
            checks.push(CheckPlan::Predicate(self.predicate(predicate)));
        }
        checks
    }

    fn kind_check(&mut self, constraint: NodeKindConstraint) -> Option<KindCheck> {
        let (class, id) = match constraint {
            NodeKindConstraint::Any => return None,
            NodeKindConstraint::Named(id) => (KindClass::Named, id),
            NodeKindConstraint::Anonymous(id) => (KindClass::Anonymous, id),
        };
        let name = id.map(|id| self.kind_name(id));
        if let (Some(id), Some(name)) = (id, &name)
            && id != NodeKindId::ERROR
        {
            let raw = u16::from(id);
            self.expected_kinds.insert(
                raw,
                ExpectedKind {
                    id: raw,
                    name: name.clone(),
                    named: class == KindClass::Named,
                },
            );
        }
        Some(KindCheck {
            class,
            id: id.map(u16::from),
            name,
        })
    }

    fn record_field(&mut self, field: NodeFieldId) -> FieldPlan {
        let field = FieldPlan {
            id: u16::from(field),
            name: self.field_name(field),
        };
        if !self.expected_fields.contains_key(&field.id) {
            self.fields.push(field.clone());
        }
        self.expected_fields.insert(
            field.id,
            ExpectedField {
                id: field.id,
                name: field.name.clone(),
            },
        );
        field
    }

    fn kind_name(&self, id: NodeKindId) -> String {
        if id == NodeKindId::ERROR {
            return "ERROR".to_string();
        }
        self.artifacts
            .grammar
            .kind_name(id, self.artifacts.interner)
            .expect("linked query binds every referenced node kind")
    }

    fn field_name(&self, id: NodeFieldId) -> String {
        self.artifacts
            .grammar
            .field_name(id, self.artifacts.interner)
            .expect("linked query binds every referenced field")
    }

    fn predicate(&mut self, predicate: &PredicateIR) -> PredicatePlan {
        let value = match &predicate.value {
            PredicateValueIR::String(value) => PredicateValuePlan::String(value.to_string()),
            PredicateValueIR::Regex(pattern) => {
                let id = if let Some(&id) = self.regex_ids.get(pattern.as_ref()) {
                    id
                } else {
                    let id = RegexId(self.regexes.len());
                    let pattern = pattern.to_string();
                    self.regex_ids.insert(pattern.clone(), id);
                    let normalized = normalize(&pattern);
                    self.regexes.push(RegexPlan {
                        id,
                        pattern: pattern.clone(),
                        normalized,
                    });
                    id
                };
                PredicateValuePlan::Regex {
                    id,
                    pattern: pattern.to_string(),
                }
            }
        };
        PredicatePlan {
            op: predicate.op,
            value,
        }
    }

    fn effects(&self, effects: &[EffectIR]) -> Vec<EffectPlan> {
        effects
            .iter()
            .map(|effect| EffectPlan {
                kind: effect.kind(),
                payload: self.effect_payload(effect),
                display: self.dumper.effect_display(effect),
            })
            .collect()
    }

    fn effect_payload(&self, effect: &EffectIR) -> u16 {
        match effect.payload() {
            EffectArg::Literal(value) => {
                u16::try_from(*value).expect("literal effect payload fits u16")
            }
            EffectArg::Member(member) => self
                .layout
                .scope(member.parent_type)
                .expect("effect member parent has a capture scope")
                .absolute_index(member.relative_index),
        }
    }

    fn flow(&self, successors: &[Label]) -> FlowPlan {
        match successors {
            [] => FlowPlan::Accept,
            [next] => FlowPlan::Jump(resolve_state(self.ids, *next)),
            [next, alternatives @ ..] => FlowPlan::Branch {
                next: resolve_state(self.ids, *next),
                alternatives: alternatives
                    .iter()
                    .map(|&label| resolve_state(self.ids, label))
                    .collect(),
            },
        }
    }
}

fn resolve_state(ids: &BTreeMap<Label, StateId>, label: Label) -> StateId {
    *ids.get(&label)
        .expect("every successor label addresses an instruction")
}
