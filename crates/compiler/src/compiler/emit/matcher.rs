use std::collections::{BTreeMap, BTreeSet, HashMap};

use plotnik_runtime::{Nav, SkipPolicy};
use regex_syntax::hir::Hir;

use crate::bytecode::{EffectKind, EntryBoundary, NodeKindConstraint, PredicateOp};
use crate::compiler::analyze::AnalysisArtifacts;
use crate::compiler::analyze::result::CaptureLayout;
use crate::compiler::ids::DefId;
use crate::compiler::lower::dump::NfaDumper;
use crate::compiler::lower::ir::{
    CallIR, EffectArg, EffectIR, InstructionIR, Label, LabelOrigin, MatchIR, NfaGraph, PortId,
    PredicateIR, PredicateValueIR,
};
use crate::compiler::regex::normalize;
use crate::core::{NodeFieldId, NodeKindId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct StateId(u16);

impl StateId {
    pub(crate) fn raw(self) -> u16 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RegexId(usize);

impl RegexId {
    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StatePlan {
    pub(crate) id: StateId,
    pub(crate) label: Label,
    pub(crate) owner: DefId,
    pub(crate) provenance: String,
    pub(crate) kind: StatePlanKind,
}

impl StatePlan {
    pub(crate) fn flows(&self) -> Vec<&FlowPlan> {
        match &self.kind {
            StatePlanKind::Switch(cases) => cases.iter().map(|case| &case.action.flow).collect(),
            StatePlanKind::Epsilon { flow, .. } => vec![flow],
            StatePlanKind::Match(plan) => vec![&plan.action.flow],
            StatePlanKind::Call(_) | StatePlanKind::Return(_) => Vec::new(),
        }
    }

    fn straight_successor(&self) -> Option<StateId> {
        match &self.kind {
            StatePlanKind::Epsilon {
                flow: FlowPlan::Jump(next),
                ..
            } => Some(*next),
            StatePlanKind::Match(MatchPlan {
                retry: None,
                action:
                    MatchActionPlan {
                        flow: FlowPlan::Jump(next),
                        ..
                    },
                ..
            }) => Some(*next),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct DefinitionPlan {
    pub(crate) owner: DefId,
    pub(crate) blocks: Vec<BlockPlan>,
}

#[derive(Clone, Debug)]
pub(crate) struct BlockPlan {
    pub(crate) id: u32,
    pub(crate) kind: BlockKind,
}

#[derive(Clone, Debug)]
pub(crate) enum BlockKind {
    States(Vec<StateId>),
    Retry(StateId),
}

#[derive(Clone, Debug)]
pub(crate) enum StatePlanKind {
    Switch(Vec<SwitchActionPlan>),
    Epsilon {
        effects: Vec<EffectPlan>,
        flow: FlowPlan,
    },
    Match(MatchPlan),
    Call(CallPlan),
    Return(PortId),
}

#[derive(Clone, Debug)]
pub(crate) struct SwitchActionPlan {
    pub(crate) kind: NodeKindId,
    pub(crate) prefix: Vec<Vec<EffectPlan>>,
    pub(crate) action: MatchActionPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct MatchPlan {
    pub(crate) nav: Nav,
    pub(crate) search: SkipPolicy,
    pub(crate) retry: Option<SkipPolicy>,
    pub(crate) kind: Option<KindCheck>,
    pub(crate) action: MatchActionPlan,
}

#[derive(Clone, Debug)]
pub(crate) struct MatchActionPlan {
    pub(crate) checks: Vec<CheckPlan>,
    pub(crate) effects: Vec<EffectPlan>,
    pub(crate) flow: FlowPlan,
}

impl MatchPlan {
    pub(crate) fn navigates(&self) -> bool {
        !matches!(self.nav, Nav::Stay | Nav::StayExact)
    }

    pub(crate) fn has_candidate_checks(&self) -> bool {
        self.kind.is_some() || !self.action.checks.is_empty()
    }

    pub(crate) fn needs_node_binding(&self) -> bool {
        self.kind.is_some() || self.action.checks.iter().any(CheckPlan::reads_node)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CallOwnership {
    Caller,
    Callee,
}

#[derive(Clone, Debug)]
pub(crate) struct CallPlan {
    pub(crate) ownership: CallOwnership,
    pub(crate) nav: Nav,
    pub(crate) search: SkipPolicy,
    pub(crate) retry: Option<SkipPolicy>,
    pub(crate) field: Option<NodeFieldId>,
    pub(crate) target: StateId,
    pub(crate) returns: Vec<StateId>,
}

impl CallPlan {
    pub(crate) fn caller_owned(&self) -> bool {
        self.ownership == CallOwnership::Caller
    }

    pub(crate) fn stays_on_current_node(&self) -> bool {
        matches!(self.nav, Nav::Stay | Nav::StayExact)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum FlowPlan {
    Accept,
    Jump(StateId),
    Fork {
        next: StateId,
        successors: Vec<StateId>,
    },
}

impl FlowPlan {
    fn targets(&self) -> impl Iterator<Item = StateId> + '_ {
        let (next, rest): (Option<StateId>, &[StateId]) = match self {
            Self::Accept => (None, &[]),
            Self::Jump(next) => (Some(*next), &[]),
            Self::Fork { next, successors } => (Some(*next), successors),
        };
        next.into_iter().chain(rest.iter().copied())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EffectPlan {
    pub(crate) kind: EffectKind,
    pub(crate) payload: u16,
}

#[derive(Clone, Debug)]
pub(crate) enum CheckPlan {
    Class(KindClass),
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
    pub(crate) id: NodeKindId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FieldPlan {
    pub(crate) id: NodeFieldId,
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
    Regex { id: RegexId },
}

#[derive(Clone, Debug)]
pub(crate) struct ExpectedKind {
    pub(crate) id: NodeKindId,
    pub(crate) name: String,
    pub(crate) named: bool,
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
    pub(crate) boundary: EntryBoundary,
}

#[derive(Clone, Debug)]
pub(crate) struct MatcherPlan {
    states: Vec<StatePlan>,
    definitions: Vec<DefinitionPlan>,
    state_blocks: BTreeMap<StateId, u32>,
    retry_blocks: BTreeMap<StateId, u32>,
    entrypoints: Vec<EntryPlan>,
    expected_kinds: Vec<ExpectedKind>,
    fields: Vec<FieldPlan>,
    regexes: Vec<RegexPlan>,
}

type BlockPartition = (
    Vec<DefinitionPlan>,
    BTreeMap<StateId, u32>,
    BTreeMap<StateId, u32>,
);

impl MatcherPlan {
    pub(super) fn build(
        graph: &NfaGraph,
        artifacts: AnalysisArtifacts<'_>,
        layout: &CaptureLayout,
    ) -> Result<Self, String> {
        let dumper = NfaDumper::new(graph, artifacts, layout);
        let mut sorted: Vec<&InstructionIR> = graph.instructions().iter().collect();
        sorted.sort_by_key(|instruction| instruction.label());
        if sorted.len() > u16::MAX as usize + 1 {
            return Err(format!(
                "generated matcher has {} states (max {})",
                sorted.len(),
                u16::MAX as usize + 1
            ));
        }

        let ids = sorted
            .iter()
            .enumerate()
            .map(|(index, instruction)| {
                let raw = u16::try_from(index).expect("validated state count fits u16 ids");
                (instruction.label(), StateId(raw))
            })
            .collect::<BTreeMap<_, _>>();
        let entrypoints: Vec<_> = graph
            .entrypoints()
            .iter()
            .map(|(&key, entry)| {
                let crate::compiler::lower::ir::EntrypointKey::Named(definition) = key else {
                    unreachable!("source generation admits only named modules");
                };
                let symbol = artifacts.definitions.definition(definition).name();
                EntryPlan {
                    definition,
                    name: artifacts.interner.resolve(symbol).to_string(),
                    entry: resolve_state(&ids, entry.target),
                    boundary: entry.boundary,
                }
            })
            .collect();

        let mut builder = MatcherPlanBuilder::new(graph, &dumper, artifacts, &ids);
        let states: Vec<_> = sorted
            .into_iter()
            .enumerate()
            .map(|(index, instruction)| builder.state(graph, index, instruction))
            .collect();

        let (definitions, state_blocks, retry_blocks) = Self::partition(&states, &entrypoints);

        Ok(Self {
            states,
            definitions,
            state_blocks,
            retry_blocks,
            entrypoints,
            expected_kinds: builder.expected_kinds.into_values().collect(),
            fields: builder.fields,
            regexes: builder.regexes,
        })
    }

    fn partition(states: &[StatePlan], entries: &[EntryPlan]) -> BlockPartition {
        let state = |id: StateId| &states[usize::from(id.raw())];
        let mut groups = BTreeMap::<DefId, Vec<StateId>>::new();
        let mut incoming = vec![0usize; states.len()];
        let mut boundaries = BTreeSet::new();
        for current in states {
            groups.entry(current.owner).or_default().push(current.id);
            for flow in current.flows() {
                for target in flow.targets() {
                    assert_eq!(
                        current.owner,
                        state(target).owner,
                        "ordinary edge {:?} -> {:?} crosses definition ownership",
                        current.label,
                        state(target).label
                    );
                    incoming[usize::from(target.raw())] += 1;
                    if current.straight_successor().is_none() {
                        boundaries.insert(target);
                    }
                }
            }
            if let StatePlanKind::Call(call) = &current.kind {
                boundaries.insert(call.target);
                for &target in &call.returns {
                    assert_eq!(
                        current.owner,
                        state(target).owner,
                        "call return {:?} -> {:?} crosses definition ownership",
                        current.label,
                        state(target).label
                    );
                    boundaries.insert(target);
                }
            }
        }
        for entry in entries {
            assert_eq!(
                entry.definition,
                state(entry.entry).owner,
                "entrypoint must belong to its definition runner"
            );
            boundaries.insert(entry.entry);
        }
        for current in states {
            if incoming[usize::from(current.id.raw())] != 1 {
                boundaries.insert(current.id);
            }
        }

        let mut definitions = Vec::new();
        let mut state_blocks = BTreeMap::new();
        let mut retry_blocks = BTreeMap::new();
        let mut covered = 0;
        for (owner, ids) in groups {
            let mut blocks = Vec::new();
            for &start in ids.iter().filter(|id| boundaries.contains(id)) {
                let id = u32::try_from(blocks.len()).expect("local block count fits u32");
                state_blocks.insert(start, id);
                let mut chain = Vec::new();
                let mut next = start;
                loop {
                    chain.push(next);
                    let Some(target) = state(next).straight_successor() else {
                        break;
                    };
                    if boundaries.contains(&target) {
                        break;
                    }
                    next = target;
                }
                covered += chain.len();
                blocks.push(BlockPlan {
                    id,
                    kind: BlockKind::States(chain),
                });
            }
            for &id in &ids {
                let retry = match &state(id).kind {
                    StatePlanKind::Match(plan) => plan.retry.is_some(),
                    StatePlanKind::Call(plan) => plan.retry.is_some(),
                    _ => false,
                };
                if retry {
                    let block =
                        u32::try_from(blocks.len()).expect("local retry block count fits u32");
                    retry_blocks.insert(id, block);
                    blocks.push(BlockPlan {
                        id: block,
                        kind: BlockKind::Retry(id),
                    });
                }
            }
            definitions.push(DefinitionPlan { owner, blocks });
        }
        assert_eq!(covered, states.len(), "local blocks cover every state");
        (definitions, state_blocks, retry_blocks)
    }

    pub(crate) fn definitions(&self) -> &[DefinitionPlan] {
        &self.definitions
    }

    pub(crate) fn state(&self, id: StateId) -> &StatePlan {
        &self.states[usize::from(id.raw())]
    }

    pub(crate) fn block(&self, id: StateId) -> u32 {
        *self
            .state_blocks
            .get(&id)
            .expect("resume target starts a local block")
    }

    pub(crate) fn retry_block(&self, id: StateId) -> u32 {
        *self
            .retry_blocks
            .get(&id)
            .expect("retryable state has a local retry block")
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

    pub(crate) fn fields(&self) -> &[FieldPlan] {
        &self.fields
    }

    pub(crate) fn regexes(&self) -> &[RegexPlan] {
        &self.regexes
    }
}

struct MatcherPlanBuilder<'p, 'a> {
    dumper: &'p NfaDumper<'a>,
    instructions: HashMap<Label, &'a InstructionIR>,
    artifacts: AnalysisArtifacts<'a>,
    ids: &'p BTreeMap<Label, StateId>,
    expected_kinds: BTreeMap<NodeKindId, ExpectedKind>,
    field_indices: BTreeMap<NodeFieldId, usize>,
    fields: Vec<FieldPlan>,
    regex_ids: BTreeMap<String, RegexId>,
    regexes: Vec<RegexPlan>,
}

impl<'p, 'a> MatcherPlanBuilder<'p, 'a> {
    fn new(
        graph: &'a NfaGraph,
        dumper: &'p NfaDumper<'a>,
        artifacts: AnalysisArtifacts<'a>,
        ids: &'p BTreeMap<Label, StateId>,
    ) -> Self {
        Self {
            dumper,
            instructions: graph
                .instructions()
                .iter()
                .map(|instruction| (instruction.label(), instruction))
                .collect(),
            artifacts,
            ids,
            expected_kinds: BTreeMap::new(),
            field_indices: BTreeMap::new(),
            fields: Vec::new(),
            regex_ids: BTreeMap::new(),
            regexes: Vec::new(),
        }
    }

    fn state(&mut self, graph: &NfaGraph, index: usize, instruction: &InstructionIR) -> StatePlan {
        let label = instruction.label();
        let owner = match graph
            .origin(label)
            .expect("every pre-pack label carries an origin")
        {
            LabelOrigin::Expression => {
                unreachable!("execution expressions are not source-generated")
            }
            LabelOrigin::Def(definition) => definition,
            LabelOrigin::DefSpecialization { def_id, .. } => def_id,
        };
        let kind = match instruction {
            InstructionIR::Switch(switch) => {
                let mut entries = switch.entries(&self.instructions);
                entries.sort_by_key(|entry| entry.target);
                StatePlanKind::Switch(
                    entries
                        .into_iter()
                        .map(|entry| SwitchActionPlan {
                            kind: entry.kind,
                            prefix: entry
                                .prefix
                                .iter()
                                .map(|instruction| self.effects(&instruction.effects))
                                .collect(),
                            action: self.match_action(entry.matched),
                        })
                        .collect(),
                )
            }
            InstructionIR::Match(instruction) => self.match_state(instruction),
            InstructionIR::Call(instruction) => StatePlanKind::Call(self.call_state(instruction)),
            InstructionIR::Return(return_) => StatePlanKind::Return(return_.port),
        };
        StatePlan {
            id: StateId(u16::try_from(index).expect("validated state count fits u16 ids")),
            label,
            owner,
            provenance: self.dumper.render_instruction(instruction),
            kind,
        }
    }

    fn match_state(&mut self, instruction: &MatchIR) -> StatePlanKind {
        if instruction.is_epsilon() {
            assert!(
                matches!(instruction.node_kind, NodeKindConstraint::Any)
                    && !instruction.missing
                    && instruction.node_field.is_none()
                    && instruction.neg_fields.is_empty()
                    && instruction.predicate.is_none(),
                "epsilon match carries no candidate checks"
            );
            return StatePlanKind::Epsilon {
                effects: self.effects(&instruction.effects),
                flow: self.flow(&instruction.successors),
            };
        }

        let search = instruction.nav.skip_policy();
        let retry = instruction.nav.is_sibling_search().then_some(search);
        StatePlanKind::Match(MatchPlan {
            nav: instruction.nav,
            search,
            retry,
            kind: self.kind_check(instruction.node_kind),
            action: self.match_action(instruction),
        })
    }

    fn match_action(&mut self, instruction: &MatchIR) -> MatchActionPlan {
        MatchActionPlan {
            checks: self.checks(instruction),
            effects: self.effects(&instruction.effects),
            flow: self.flow(&instruction.successors),
        }
    }

    fn call_state(&mut self, instruction: &CallIR) -> CallPlan {
        let target = resolve_state(self.ids, instruction.target);
        let nav = instruction.entry.nav();
        let search = nav.skip_policy();
        let caller_owned = instruction.entry.caller_owned();
        let stays = matches!(nav, Nav::Stay | Nav::StayExact);
        let retry = (caller_owned && !stays && search != SkipPolicy::Exact).then_some(search);
        let field = instruction
            .entry
            .field()
            .map(|field| self.record_field(field).id);
        let returns = instruction
            .return_labels()
            .iter()
            .copied()
            .map(|label| resolve_state(self.ids, label))
            .collect::<Vec<_>>();
        assert!(
            !returns.is_empty() && returns.len() <= usize::from(PortId::COUNT),
            "call plan must provide 1..={} continuations",
            PortId::COUNT
        );

        CallPlan {
            ownership: if caller_owned {
                CallOwnership::Caller
            } else {
                CallOwnership::Callee
            },
            nav,
            search,
            retry,
            field,
            target,
            returns,
        }
    }

    fn checks(&mut self, instruction: &MatchIR) -> Vec<CheckPlan> {
        let mut checks = Vec::new();
        if !instruction.node_kind.is_any() {
            let class = if instruction.node_kind.is_named() {
                KindClass::Named
            } else {
                KindClass::Anonymous
            };
            checks.push(CheckPlan::Class(class));
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
            checks.push(CheckPlan::Predicate(self.predicate(predicate)));
        }
        checks
    }

    fn kind_check(&mut self, constraint: NodeKindConstraint) -> Option<KindCheck> {
        let id = constraint.kind_id()?;
        if id != NodeKindId::ERROR {
            self.expected_kinds.insert(
                id,
                ExpectedKind {
                    id,
                    name: self.kind_name(id),
                    named: constraint.is_named(),
                },
            );
        }
        Some(KindCheck { id })
    }

    fn record_field(&mut self, field: NodeFieldId) -> FieldPlan {
        if let Some(&index) = self.field_indices.get(&field) {
            return self.fields[index].clone();
        }

        let plan = FieldPlan {
            id: field,
            name: self.field_name(field),
        };
        self.field_indices.insert(field, self.fields.len());
        self.fields.push(plan.clone());
        plan
    }

    fn kind_name(&self, id: NodeKindId) -> String {
        if id == NodeKindId::ERROR {
            return "ERROR".to_string();
        }
        self.artifacts
            .grammar
            .kind_name(id, self.artifacts.interner)
            .expect("grammar-bound query binds every referenced node kind")
    }

    fn field_name(&self, id: NodeFieldId) -> String {
        self.artifacts
            .grammar
            .field_name(id, self.artifacts.interner)
            .expect("grammar-bound query binds every referenced field")
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
                        pattern,
                        normalized,
                    });
                    id
                };
                PredicateValuePlan::Regex { id }
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
            })
            .collect()
    }

    fn effect_payload(&self, effect: &EffectIR) -> u16 {
        match effect.payload() {
            EffectArg::Literal(value) => {
                u16::try_from(*value).expect("literal effect payload fits u16")
            }
            EffectArg::Member(member) => member.raw(),
        }
    }

    fn flow(&self, successors: &[Label]) -> FlowPlan {
        match successors {
            [] => FlowPlan::Accept,
            [next] => FlowPlan::Jump(resolve_state(self.ids, *next)),
            [next, successors @ ..] => FlowPlan::Fork {
                next: resolve_state(self.ids, *next),
                successors: successors
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
