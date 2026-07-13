//! Always-on verification at the executor fork point.
//!
//! This is the production trust boundary for lowered query IR. It mirrors the
//! bytecode validator's collecting effect-stack analysis, but works before any
//! target chooses a representation.

#[cfg(test)]
use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::bytecode::{EffectKind, FrameAction, ValueFrameKind};
use crate::compiler::analyze::output::{CaptureMemberKind, CaptureScopeKind, OutputSchema};
use crate::compiler::analyze::types::type_shape::{TYPE_NO_VALUE, TypeShape};
use crate::compiler::lower::ir::{
    CallProtocol, DefRoute, EffectArg, EffectIR, InstructionIR, Label, MemberRef, NfaGraph,
    ReturnEntry, ReturnOutcome, SemanticNfa,
};

const STATE_BUDGET: usize = 1 << 18;
pub(crate) const MAX_STATES: usize = u16::MAX as usize + 1;

#[cfg(test)]
thread_local! {
    static BODY_ANALYSES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(crate) fn reset_body_analyses() {
    BODY_ANALYSES.set(0);
}

#[cfg(test)]
pub(crate) fn body_analyses() -> usize {
    BODY_ANALYSES.get()
}

#[cfg(test)]
fn record_body_analysis() {
    BODY_ANALYSES.set(BODY_ANALYSES.get() + 1);
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SemanticVerifyError {
    #[error("semantic matcher has {0} states (max {MAX_STATES})")]
    StateLimit(usize),
    #[error("semantic matcher has {0} entry points (max {max})", max = u16::MAX)]
    EntryPointLimit(usize),
    #[error("malformed semantic NFA: {0}")]
    Malformed(String),
    #[error("effect stack is imbalanced at state {0:?}")]
    EffectStack(Label),
    #[error("inspection span stack is imbalanced at state {0:?}")]
    SpanStack(Label),
    #[error("semantic verification state budget exceeded at state {0:?}")]
    StateBudget(Label),
    #[error("capture member reference is invalid at state {state:?}: {detail}")]
    CaptureMember { state: Label, detail: String },
    #[error("cursor depth is imbalanced: {0}")]
    CursorDepth(String),
    #[error("cursor-reading effect is reachable on a zero-width path at state {0:?}")]
    ZeroWidthCursorRead(Label),
    #[error("native regex DFA compilation failed for `{pattern}`: {error}")]
    Regex { pattern: String, error: String },
}

pub(crate) fn verify(
    semantic: &SemanticNfa,
    schema: &OutputSchema<'_>,
) -> Result<(), SemanticVerifyError> {
    verify_state_count(semantic)?;

    let graph = semantic.raw();
    if graph.entry_point_wrappers().len() > u16::MAX as usize {
        return Err(SemanticVerifyError::EntryPointLimit(
            graph.entry_point_wrappers().len(),
        ));
    }
    verify_regexes(graph)?;
    let program = Program::new(graph, schema)?;
    program.verify_cursor_depth()?;
    program.verify_zero_width_cursor_reads()?;
    program.verify_effects()
}

fn verify_regexes(graph: &NfaGraph) -> Result<(), SemanticVerifyError> {
    for instruction in graph.instructions() {
        let InstructionIR::Match(matched) = instruction else {
            continue;
        };
        let Some(predicate) = &matched.predicate else {
            continue;
        };
        let crate::compiler::lower::ir::PredicateValueIR::Regex(pattern) = &predicate.value else {
            continue;
        };
        let normalized = crate::compiler::regex::normalize(pattern);
        crate::compiler::regex::compile_native_dfa(&normalized).map_err(|error| {
            SemanticVerifyError::Regex {
                pattern: pattern.to_string(),
                error,
            }
        })?;
    }
    Ok(())
}

fn verify_state_count(semantic: &SemanticNfa) -> Result<(), SemanticVerifyError> {
    let count = semantic.raw().instructions().len();
    if count > MAX_STATES {
        return Err(SemanticVerifyError::StateLimit(count));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::{MAX_STATES, SemanticVerifyError, verify_state_count};
    use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR, NfaGraph, SemanticNfa};

    #[test]
    fn semantic_state_id_boundary_matches_codegen_dense_ids() {
        let accepted = semantic_with_states(MAX_STATES);
        verify_state_count(&accepted).expect("u16::MAX + 1 dense states fit ids 0..=u16::MAX");

        let rejected = semantic_with_states(MAX_STATES + 1);
        assert_eq!(
            verify_state_count(&rejected),
            Err(SemanticVerifyError::StateLimit(MAX_STATES + 1))
        );
    }

    fn semantic_with_states(count: usize) -> SemanticNfa {
        let instructions = (0..count)
            .map(|index| {
                InstructionIR::Match(MatchIR::terminal(Label(
                    u32::try_from(index).expect("test state count fits u32 labels"),
                )))
            })
            .collect();
        SemanticNfa::new(NfaGraph {
            instructions,
            def_entries: IndexMap::new(),
            entry_point_wrappers: IndexMap::new(),
            spans: None,
            label_origins: vec![None; count],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FrameKind {
    List,
    Record,
    Variant {
        has_no_payload: bool,
        got_data: bool,
    },
    Scalar,
}

impl FrameKind {
    fn bit(self) -> u8 {
        match self {
            Self::List => KS_LIST,
            Self::Record => KS_RECORD,
            Self::Variant { .. } => KS_VARIANT,
            Self::Scalar => KS_SCALAR,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum PendingState {
    Empty,
    Full,
    Unknown,
}

impl PendingState {
    fn from_bool(value: bool) -> Self {
        if value { Self::Full } else { Self::Empty }
    }

    fn known(self) -> Option<bool> {
        match self {
            Self::Empty => Some(false),
            Self::Full => Some(true),
            Self::Unknown => None,
        }
    }
}

const KS_LIST: u8 = 0b001;
const KS_RECORD: u8 = 0b010;
const KS_VARIANT: u8 = 0b100;
const KS_SCALAR: u8 = 0b1000;
const KS_ANY: u8 = KS_LIST | KS_RECORD | KS_VARIANT | KS_SCALAR;
const KS_RECORD_SET_TARGET: u8 = KS_RECORD | KS_VARIANT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefSummary {
    entry_tos: u8,
    returns_pending: Option<bool>,
    record_sets_caller_top: bool,
}

impl DefSummary {
    fn unknown() -> Self {
        Self {
            entry_tos: KS_ANY,
            returns_pending: None,
            record_sets_caller_top: false,
        }
    }

    /// Accumulate the monotone facts learned from one dependency state. A
    /// known pending result cannot flip without contradicting two body exits.
    fn refine(self, next: Self) -> Option<Self> {
        let returns_pending = match (self.returns_pending, next.returns_pending) {
            (Some(old), Some(new)) if old != new => return None,
            (Some(value), _) | (_, Some(value)) => Some(value),
            (None, None) => None,
        };
        Some(Self {
            entry_tos: self.entry_tos & next.entry_tos,
            returns_pending,
            record_sets_caller_top: self.record_sets_caller_top | next.record_sets_caller_top,
        })
    }
}

type DefSummaries = HashMap<Label, DefSummary>;

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReturnOutcomes(u8);

impl ReturnOutcomes {
    const NONE: Self = Self(0);
    const MATCHED: Self = Self(1);
    const BOTH: Self = Self(3);

    fn insert(&mut self, outcome: crate::compiler::lower::ir::ReturnOutcome) {
        self.0 |= match outcome {
            crate::compiler::lower::ir::ReturnOutcome::Matched => Self::MATCHED.0,
            crate::compiler::lower::ir::ReturnOutcome::Zero => 2,
        };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReturnContract {
    outcomes: ReturnOutcomes,
    entry: Option<ReturnEntry>,
    mixed_entries: bool,
}

impl ReturnContract {
    const NONE: Self = Self {
        outcomes: ReturnOutcomes::NONE,
        entry: None,
        mixed_entries: false,
    };

    fn insert(&mut self, return_: &crate::compiler::lower::ir::ReturnIR) {
        self.outcomes.insert(return_.outcome());
        match self.entry {
            None => self.entry = Some(return_.entry()),
            Some(entry) if entry != return_.entry() => self.mixed_entries = true,
            Some(_) => {}
        }
    }

    fn is(self, outcomes: ReturnOutcomes, entry: ReturnEntry) -> bool {
        self.outcomes == outcomes && self.entry == Some(entry) && !self.mixed_entries
    }
}

struct Program<'a> {
    graph: &'a NfaGraph,
    schema: &'a OutputSchema<'a>,
    instructions: HashMap<Label, &'a InstructionIR>,
}

impl<'a> Program<'a> {
    fn new(graph: &'a NfaGraph, schema: &'a OutputSchema<'a>) -> Result<Self, SemanticVerifyError> {
        let mut instructions = HashMap::with_capacity(graph.instructions().len());
        for instruction in graph.instructions() {
            if instructions
                .insert(instruction.label(), instruction)
                .is_some()
            {
                return Err(SemanticVerifyError::Malformed(format!(
                    "duplicate label {:?}",
                    instruction.label()
                )));
            }
        }
        for instruction in graph.instructions() {
            for successor in instruction.successors() {
                if !instructions.contains_key(successor) {
                    return Err(SemanticVerifyError::Malformed(format!(
                        "dangling successor {successor:?} from {:?}",
                        instruction.label()
                    )));
                }
            }
            if let InstructionIR::Call(call) = instruction
                && !instructions.contains_key(&call.target)
            {
                return Err(SemanticVerifyError::Malformed(format!(
                    "dangling call target {:?} from {:?}",
                    call.target, call.label
                )));
            }
        }
        let program = Self {
            graph,
            schema,
            instructions,
        };
        program.verify_return_routes()?;
        Ok(program)
    }

    fn verify_return_routes(&self) -> Result<(), SemanticVerifyError> {
        let mut cache = HashMap::new();
        for &entry in self.graph.entry_point_wrappers().values() {
            if !self
                .return_contract(entry, &mut cache)
                .is(ReturnOutcomes::MATCHED, ReturnEntry::Caller)
            {
                return Err(SemanticVerifyError::Malformed(format!(
                    "entry point {entry:?} has the wrong return contract"
                )));
            }
        }
        for instruction in self.graph.instructions() {
            let InstructionIR::Call(call) = instruction else {
                continue;
            };
            let expected = match call.protocol {
                CallProtocol::Ordinary { .. } => (ReturnOutcomes::MATCHED, ReturnEntry::Caller),
                CallProtocol::Routed { .. } => (ReturnOutcomes::MATCHED, ReturnEntry::Routed),
                CallProtocol::Split { .. } => (ReturnOutcomes::BOTH, ReturnEntry::Routed),
            };
            if !self
                .return_contract(call.target, &mut cache)
                .is(expected.0, expected.1)
            {
                return Err(SemanticVerifyError::Malformed(format!(
                    "call {:?} and callee {:?} disagree on return outcomes",
                    call.label, call.target
                )));
            }
        }
        Ok(())
    }

    fn return_contract(
        &self,
        entry: Label,
        cache: &mut HashMap<Label, ReturnContract>,
    ) -> ReturnContract {
        if let Some(&contract) = cache.get(&entry) {
            return contract;
        }

        let mut contract = ReturnContract::NONE;
        let mut seen = HashSet::new();
        let mut work = vec![entry];
        while let Some(label) = work.pop() {
            if !seen.insert(label) {
                continue;
            }
            match self.instructions[&label] {
                InstructionIR::Match(matched) => work.extend(&matched.successors),
                InstructionIR::Call(call) => work.extend(call.return_labels()),
                InstructionIR::Return(return_) => contract.insert(return_),
            }
        }
        cache.insert(entry, contract);
        contract
    }

    fn verify_effects(&self) -> Result<(), SemanticVerifyError> {
        let wrappers: Vec<Label> = self
            .graph
            .entry_point_wrappers()
            .values()
            .copied()
            .collect();
        let mut definitions = Vec::new();
        let mut known = HashSet::new();
        let mut called = HashSet::new();
        let mut summaries = DefSummaries::new();
        let mut queue = VecDeque::new();
        let mut queued = HashSet::new();
        // Revisit only direct callers whose callee summary changed. FIFO
        // scheduling coalesces a batch of leaf changes before a wide caller.
        let mut callers: HashMap<Label, Vec<Label>> = HashMap::new();
        let mut call_edges = HashSet::new();

        for &entry in &wrappers {
            if known.insert(entry) {
                definitions.push(entry);
                summaries.insert(entry, DefSummary::unknown());
                enqueue(&mut queue, &mut queued, entry);
            }
        }

        while let Some(entry) = queue.pop_front() {
            queued.remove(&entry);

            let analysis = self.analyze_body(
                &summaries,
                entry,
                BodyRole::from_called(called.contains(&entry)),
                VerifyPhase::Summarize,
            )?;
            for target in analysis.discovered {
                if known.insert(target) {
                    definitions.push(target);
                    summaries.insert(target, DefSummary::unknown());
                    enqueue(&mut queue, &mut queued, target);
                }
                if called.insert(target) {
                    enqueue(&mut queue, &mut queued, target);
                }
                if call_edges.insert((entry, target)) {
                    callers.entry(target).or_default().push(entry);
                }
            }

            let analysis_summary = DefSummary {
                entry_tos: analysis.entry_tos,
                returns_pending: analysis.returns_pending,
                record_sets_caller_top: analysis.record_sets_caller_top,
            };
            let old = summaries[&entry];
            let Some(next) = old.refine(analysis_summary) else {
                return Err(SemanticVerifyError::EffectStack(entry));
            };
            if old == next {
                continue;
            }
            summaries.insert(entry, next);
            if let Some(dependents) = callers.get(&entry) {
                for &caller in dependents {
                    enqueue(&mut queue, &mut queued, caller);
                }
            }
        }

        for &entry in &definitions {
            self.analyze_body(
                &summaries,
                entry,
                BodyRole::from_called(called.contains(&entry)),
                VerifyPhase::Final,
            )?;
        }
        for entry in wrappers {
            let wrapper = self.analyze_body(
                &summaries,
                entry,
                BodyRole::from_called(called.contains(&entry)),
                VerifyPhase::Final,
            )?;
            if wrapper.entry_tos != KS_ANY {
                return Err(SemanticVerifyError::EffectStack(entry));
            }
        }
        Ok(())
    }

    fn analyze_body(
        &self,
        summaries: &DefSummaries,
        entry: Label,
        role: BodyRole,
        phase: VerifyPhase,
    ) -> Result<BodyAnalysis, SemanticVerifyError> {
        #[cfg(test)]
        record_body_analysis();

        let mut entry_tos = KS_ANY;
        let mut returns_pending = None;
        let mut record_sets_caller_top = false;
        let mut discovered = Vec::new();
        let mut discovered_set = HashSet::new();
        let mut memo: HashMap<Label, HashMap<AbsState, ()>> = HashMap::new();
        let mut states_spent = 0;
        let mut frame_openers = 0;
        let mut suppression_openers = 0;
        let mut span_openers = 0;
        let mut work = vec![(entry, AbsState::initial())];

        while let Some((label, state)) = work.pop() {
            let instruction = *self
                .instructions
                .get(&label)
                .expect("program construction validates every work label");
            let seen = memo.entry(label).or_insert_with(|| {
                if let InstructionIR::Match(matched) = instruction {
                    for effect in &matched.effects {
                        match effect.kind() {
                            EffectKind::ListOpen
                            | EffectKind::RecordOpen
                            | EffectKind::VariantOpen
                            | EffectKind::ScalarOpen => frame_openers += 1,
                            EffectKind::SuppressBegin => suppression_openers += 1,
                            EffectKind::SpanStartAt | EffectKind::SpanStart => span_openers += 1,
                            _ => {}
                        }
                    }
                }
                HashMap::new()
            });
            let Some(state) = take_unseen_state(seen, state) else {
                continue;
            };
            if state.stack.len() > frame_openers || state.suppress > suppression_openers {
                return Err(SemanticVerifyError::EffectStack(label));
            }
            if state.span_stack.len() > span_openers {
                return Err(SemanticVerifyError::SpanStack(label));
            }
            states_spent += 1;
            if states_spent > STATE_BUDGET {
                return Err(SemanticVerifyError::StateBudget(label));
            }
            if matches!(instruction, InstructionIR::Return(_)) {
                state.record_exit(&mut returns_pending, label)?;
                continue;
            }
            let AbsState {
                mut stack,
                mut suppress,
                mut span_stack,
                mut pending,
            } = state;
            match instruction {
                InstructionIR::Return(_) => unreachable!("returns exit before state mutation"),
                InstructionIR::Match(matched) => {
                    for effect in &matched.effects {
                        self.apply_effect(
                            effect,
                            EffectState {
                                stack: &mut stack,
                                suppress: &mut suppress,
                                span_stack: &mut span_stack,
                                pending: &mut pending,
                                entry_tos: &mut entry_tos,
                                record_sets_caller_top: &mut record_sets_caller_top,
                            },
                            label,
                        )?;
                    }
                    if matched.successors.is_empty() {
                        if role == BodyRole::Called || !stack.is_empty() || suppress != 0 {
                            return Err(SemanticVerifyError::EffectStack(label));
                        }
                        if !span_stack.is_empty() {
                            return Err(SemanticVerifyError::SpanStack(label));
                        }
                        continue;
                    }
                    for successor in &matched.successors {
                        work.push((
                            *successor,
                            AbsState {
                                stack: stack.clone(),
                                suppress,
                                span_stack: span_stack.clone(),
                                pending,
                            },
                        ));
                    }
                }
                InstructionIR::Call(call) => {
                    if discovered_set.insert(call.target) {
                        discovered.push(call.target);
                    }
                    if suppress > 0 {
                        let state = AbsState {
                            stack,
                            suppress,
                            span_stack,
                            pending,
                        };
                        push_call_returns(&mut work, call, state);
                        continue;
                    }
                    if pending == PendingState::Full {
                        return Err(SemanticVerifyError::EffectStack(label));
                    }
                    let summary = summaries
                        .get(&call.target)
                        .copied()
                        .unwrap_or_else(DefSummary::unknown);
                    match stack.last() {
                        Some(kind)
                            if phase == VerifyPhase::Final
                                && kind.bit() & summary.entry_tos == 0 =>
                        {
                            return Err(SemanticVerifyError::EffectStack(label));
                        }
                        None => {
                            entry_tos &= summary.entry_tos;
                            record_sets_caller_top |= summary.record_sets_caller_top;
                        }
                        Some(_) => {}
                    }
                    let post_pending = summary
                        .returns_pending
                        .map(PendingState::from_bool)
                        .unwrap_or(PendingState::Unknown);
                    if summary.record_sets_caller_top
                        && let Some(FrameKind::Variant {
                            got_data: false, ..
                        }) = stack.last()
                    {
                        let mut written = stack.clone();
                        if let Some(FrameKind::Variant { got_data, .. }) = written.last_mut() {
                            *got_data = true;
                        }
                        push_call_returns(
                            &mut work,
                            call,
                            AbsState {
                                stack: written,
                                suppress,
                                span_stack: span_stack.clone(),
                                pending: post_pending,
                            },
                        );
                    }
                    push_call_returns(
                        &mut work,
                        call,
                        AbsState {
                            stack,
                            suppress,
                            span_stack,
                            pending: post_pending,
                        },
                    );
                }
            }
        }

        Ok(BodyAnalysis {
            entry_tos,
            returns_pending,
            record_sets_caller_top,
            discovered,
        })
    }

    fn apply_effect(
        &self,
        effect: &EffectIR,
        state: EffectState<'_>,
        label: Label,
    ) -> Result<(), SemanticVerifyError> {
        use EffectKind::*;

        if *state.suppress > 0 {
            match effect.kind() {
                SuppressBegin => *state.suppress += 1,
                SuppressEnd => *state.suppress -= 1,
                SpanStartAt | SpanStart => state.span_stack.push(literal(effect, label)?),
                SpanEnd => close_span(state.span_stack, literal(effect, label)?, label)?,
                ScalarMark => {}
                _ => {}
            }
            return Ok(());
        }

        if effect.kind().frame_action().is_some() {
            return self.apply_frame_action(effect, state, label);
        }

        match effect.kind() {
            Node | Absent | NodeStr | NodeBool | BoolValue => {
                if *state.pending == PendingState::Full {
                    return Err(SemanticVerifyError::EffectStack(label));
                }
                *state.pending = PendingState::Full;
            }
            SuppressBegin => *state.suppress += 1,
            SuppressEnd => return Err(SemanticVerifyError::EffectStack(label)),
            SpanStartAt | SpanStart => state.span_stack.push(literal(effect, label)?),
            SpanEnd => close_span(state.span_stack, literal(effect, label)?, label)?,
            ScalarMark => {}
            ArrayPush => {
                require_pending(state.pending, label)?;
                match state.stack.last() {
                    Some(FrameKind::List) => {}
                    Some(_) => return Err(SemanticVerifyError::EffectStack(label)),
                    None => *state.entry_tos &= KS_LIST,
                }
                *state.pending = PendingState::Empty;
            }
            RecordSet => {
                self.validate_record_set_member(member(effect, label)?, label)?;
                require_pending(state.pending, label)?;
                match state.stack.last_mut() {
                    Some(FrameKind::Record) => {}
                    Some(FrameKind::Variant { got_data, .. }) => *got_data = true,
                    Some(FrameKind::List | FrameKind::Scalar) => {
                        return Err(SemanticVerifyError::EffectStack(label));
                    }
                    None => {
                        *state.entry_tos &= KS_RECORD_SET_TARGET;
                        *state.record_sets_caller_top = true;
                    }
                }
                *state.pending = PendingState::Empty;
            }
            ListOpen | ListClose | RecordOpen | RecordClose | VariantOpen | VariantClose
            | ScalarOpen | StrClose | BoolClose => {
                unreachable!("frame effects return before data dispatch")
            }
        }
        Ok(())
    }

    fn apply_frame_action(
        &self,
        effect: &EffectIR,
        state: EffectState<'_>,
        label: Label,
    ) -> Result<(), SemanticVerifyError> {
        let action = effect
            .kind()
            .frame_action()
            .expect("frame action effects are dispatched here");
        match action {
            FrameAction::Open(kind) => {
                let frame = match kind {
                    ValueFrameKind::List => FrameKind::List,
                    ValueFrameKind::Record => FrameKind::Record,
                    ValueFrameKind::Variant => FrameKind::Variant {
                        has_no_payload: self.case_has_no_payload(member(effect, label)?, label)?,
                        got_data: false,
                    },
                    ValueFrameKind::Scalar => FrameKind::Scalar,
                };
                open_frame(state.stack, state.pending, frame, label)?;
            }
            FrameAction::Close(ValueFrameKind::Variant) => match state.stack.pop() {
                Some(FrameKind::Variant {
                    has_no_payload,
                    got_data,
                }) => {
                    let data_pending = match *state.pending {
                        PendingState::Full => true,
                        PendingState::Empty => false,
                        PendingState::Unknown => !got_data && !has_no_payload,
                    };
                    if data_pending && got_data || (data_pending || got_data) == has_no_payload {
                        return Err(SemanticVerifyError::EffectStack(label));
                    }
                    *state.pending = PendingState::Full;
                }
                _ => return Err(SemanticVerifyError::EffectStack(label)),
            },
            FrameAction::Close(kind) => {
                let expected = match kind {
                    ValueFrameKind::List => FrameKind::List,
                    ValueFrameKind::Record => FrameKind::Record,
                    ValueFrameKind::Scalar => FrameKind::Scalar,
                    ValueFrameKind::Variant => unreachable!("variant close handled above"),
                };
                close_simple_frame(state.stack, state.pending, expected, label)?;
            }
        }
        Ok(())
    }

    fn case_has_no_payload(
        &self,
        member: MemberRef,
        label: Label,
    ) -> Result<bool, SemanticVerifyError> {
        let scope = self.member_scope(member, label)?;
        if scope.kind() != CaptureScopeKind::Variant {
            return Err(capture_error(
                label,
                "VariantOpen does not reference a variant case",
            ));
        }
        let CaptureMemberKind::Case(payload) = scope.members()[member.relative_index as usize].kind
        else {
            return Err(capture_error(
                label,
                "VariantOpen does not reference a variant case",
            ));
        };
        Ok(payload == TYPE_NO_VALUE
            || matches!(
                self.schema.types.expect_type_shape(payload),
                TypeShape::NoValue
            ))
    }

    fn validate_record_set_member(
        &self,
        member: MemberRef,
        label: Label,
    ) -> Result<(), SemanticVerifyError> {
        let scope = self.member_scope(member, label)?;
        if scope.kind() == CaptureScopeKind::Record
            && !matches!(
                scope.members()[member.relative_index as usize].kind,
                CaptureMemberKind::Field(_)
            )
        {
            return Err(capture_error(
                label,
                "RecordSet references a non-field member",
            ));
        }
        Ok(())
    }

    fn member_scope(
        &self,
        member: MemberRef,
        label: Label,
    ) -> Result<&crate::compiler::analyze::output::CaptureScope, SemanticVerifyError> {
        let Some(scope) = self.schema.layout().scope(member.parent_type) else {
            return Err(capture_error(label, "member parent has no capture scope"));
        };
        if usize::from(member.relative_index) >= scope.members().len() {
            return Err(capture_error(
                label,
                "relative member index is out of bounds",
            ));
        }
        let _ = scope.absolute_index(member.relative_index);
        Ok(scope)
    }

    fn verify_cursor_depth(&self) -> Result<(), SemanticVerifyError> {
        for (entry, route) in self.depth_entries() {
            let mut memo = HashMap::new();
            let mut work = vec![(entry, 0i32)];
            while let Some((label, depth)) = work.pop() {
                if let Some(seen) = memo.insert(label, depth) {
                    if seen != depth {
                        return Err(SemanticVerifyError::CursorDepth(format!(
                            "state {label:?} reached at depths {seen} and {depth}"
                        )));
                    }
                    continue;
                }
                match self.instructions[&label] {
                    InstructionIR::Match(matched) => {
                        let next_depth = depth + matched.nav.depth_delta();
                        let expected_exit = route
                            .return_depth(ReturnOutcome::Matched)
                            .expect("every body has a matched route");
                        if matched.successors.is_empty() && next_depth != expected_exit {
                            return Err(SemanticVerifyError::CursorDepth(format!(
                                "accepting state {label:?} exits at depth {next_depth}, expected {expected_exit}"
                            )));
                        }
                        for successor in &matched.successors {
                            work.push((*successor, next_depth));
                        }
                    }
                    InstructionIR::Call(call) => {
                        work.push((
                            call.matched_return(),
                            depth + call.entry_nav().depth_delta(),
                        ));
                        if let Some(zero) = call.zero_return() {
                            work.push((zero, depth));
                        }
                    }
                    InstructionIR::Return(return_) => {
                        if return_.entry() != route.return_entry() {
                            return Err(SemanticVerifyError::CursorDepth(format!(
                                "return state {label:?} has {:?} entry, expected {:?}",
                                return_.entry(),
                                route.return_entry()
                            )));
                        }
                        let Some(expected_exit) = route.return_depth(return_.outcome()) else {
                            return Err(SemanticVerifyError::CursorDepth(format!(
                                "return state {label:?} has unsupported {:?} outcome",
                                return_.outcome()
                            )));
                        };
                        if depth != expected_exit {
                            return Err(SemanticVerifyError::CursorDepth(format!(
                                "return state {label:?} exits at depth {depth}, expected {expected_exit}"
                            )));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn verify_zero_width_cursor_reads(&self) -> Result<(), SemanticVerifyError> {
        for entry in self.entries() {
            let mut memo: HashMap<Label, bool> = HashMap::new();
            let mut work = vec![(entry, true)];
            while let Some((label, zero_width)) = work.pop() {
                if let Some(seen_zero_width) = memo.get(&label)
                    && (*seen_zero_width || !zero_width)
                {
                    continue;
                }
                memo.insert(label, zero_width);
                match self.instructions[&label] {
                    InstructionIR::Match(matched) => {
                        let after = zero_width && matched.nav == plotnik_rt::Nav::Epsilon;
                        if after
                            && matched
                                .effects
                                .iter()
                                .any(|effect| effect.kind().reads_cursor())
                        {
                            return Err(SemanticVerifyError::ZeroWidthCursorRead(label));
                        }
                        for successor in &matched.successors {
                            work.push((*successor, after));
                        }
                    }
                    InstructionIR::Call(call) => {
                        work.push((call.matched_return(), false));
                        if let Some(zero) = call.zero_return() {
                            work.push((zero, zero_width));
                        }
                    }
                    InstructionIR::Return(_) => {}
                }
            }
        }
        Ok(())
    }

    fn entries(&self) -> impl Iterator<Item = Label> + '_ {
        self.graph
            .entry_point_wrappers
            .values()
            .chain(self.graph.def_entries.values())
            .copied()
    }

    fn depth_entries(&self) -> Vec<(Label, DefRoute)> {
        let wrappers = self
            .graph
            .entry_point_wrappers
            .values()
            .copied()
            .map(|entry| (entry, DefRoute::Caller));
        let definitions = self
            .graph
            .def_entries
            .iter()
            .map(|(variant, &entry)| (entry, variant.route()));
        wrappers.chain(definitions).collect()
    }
}

fn push_call_returns(
    work: &mut Vec<(Label, AbsState)>,
    call: &crate::compiler::lower::ir::CallIR,
    state: AbsState,
) {
    match call.protocol {
        CallProtocol::Ordinary { next, .. } | CallProtocol::Routed { next, .. } => {
            work.push((next, state));
        }
        CallProtocol::Split { returns, .. } => {
            work.push((returns[1], state.clone()));
            work.push((returns[0], state));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyRole {
    Root,
    Called,
}

impl BodyRole {
    fn from_called(called: bool) -> Self {
        if called {
            return Self::Called;
        }
        Self::Root
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerifyPhase {
    Summarize,
    Final,
}

struct BodyAnalysis {
    entry_tos: u8,
    returns_pending: Option<bool>,
    record_sets_caller_top: bool,
    discovered: Vec<Label>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct AbsState {
    stack: Vec<FrameKind>,
    suppress: usize,
    span_stack: Vec<usize>,
    pending: PendingState,
}

impl AbsState {
    fn initial() -> Self {
        Self {
            stack: Vec::new(),
            suppress: 0,
            span_stack: Vec::new(),
            pending: PendingState::Empty,
        }
    }

    fn record_exit(
        &self,
        returns_pending: &mut Option<bool>,
        label: Label,
    ) -> Result<(), SemanticVerifyError> {
        if !self.stack.is_empty() || self.suppress != 0 {
            return Err(SemanticVerifyError::EffectStack(label));
        }
        if !self.span_stack.is_empty() {
            return Err(SemanticVerifyError::SpanStack(label));
        }
        if let Some(pending) = self.pending.known() {
            if let Some(seen) = *returns_pending
                && seen != pending
            {
                return Err(SemanticVerifyError::EffectStack(label));
            }
            *returns_pending = Some(pending);
        }
        Ok(())
    }
}

/// Remember `state` and return an owned copy only on its first visit. The map's
/// entry API avoids cloning stack and span vectors for duplicate arrivals.
fn take_unseen_state(seen: &mut HashMap<AbsState, ()>, state: AbsState) -> Option<AbsState> {
    match seen.entry(state) {
        Entry::Occupied(_) => None,
        Entry::Vacant(entry) => {
            let state = entry.key().clone();
            entry.insert(());
            Some(state)
        }
    }
}

struct EffectState<'a> {
    stack: &'a mut Vec<FrameKind>,
    suppress: &'a mut usize,
    span_stack: &'a mut Vec<usize>,
    pending: &'a mut PendingState,
    entry_tos: &'a mut u8,
    record_sets_caller_top: &'a mut bool,
}

fn literal(effect: &EffectIR, label: Label) -> Result<usize, SemanticVerifyError> {
    match effect.argument() {
        EffectArg::Literal(value) => Ok(*value),
        EffectArg::Member(_) => Err(capture_error(
            label,
            "literal effect uses a member argument",
        )),
    }
}

fn member(effect: &EffectIR, label: Label) -> Result<MemberRef, SemanticVerifyError> {
    match effect.argument() {
        EffectArg::Member(member) => Ok(*member),
        EffectArg::Literal(_) => Err(capture_error(
            label,
            "member effect uses a literal argument",
        )),
    }
}

fn capture_error(label: Label, detail: impl Into<String>) -> SemanticVerifyError {
    SemanticVerifyError::CaptureMember {
        state: label,
        detail: detail.into(),
    }
}

fn open_frame(
    stack: &mut Vec<FrameKind>,
    pending: &mut PendingState,
    frame: FrameKind,
    label: Label,
) -> Result<(), SemanticVerifyError> {
    if *pending == PendingState::Full {
        return Err(SemanticVerifyError::EffectStack(label));
    }
    stack.push(frame);
    Ok(())
}

fn require_pending(pending: &PendingState, label: Label) -> Result<(), SemanticVerifyError> {
    if *pending == PendingState::Empty {
        return Err(SemanticVerifyError::EffectStack(label));
    }
    Ok(())
}

fn close_simple_frame(
    stack: &mut Vec<FrameKind>,
    pending: &mut PendingState,
    expected: FrameKind,
    label: Label,
) -> Result<(), SemanticVerifyError> {
    if stack.pop() != Some(expected) || *pending == PendingState::Full {
        return Err(SemanticVerifyError::EffectStack(label));
    }
    *pending = PendingState::Full;
    Ok(())
}

fn close_span(stack: &mut Vec<usize>, id: usize, label: Label) -> Result<(), SemanticVerifyError> {
    if stack.pop() != Some(id) {
        return Err(SemanticVerifyError::SpanStack(label));
    }
    Ok(())
}

fn enqueue(queue: &mut VecDeque<Label>, queued: &mut HashSet<Label>, entry: Label) {
    if queued.insert(entry) {
        queue.push_back(entry);
    }
}
