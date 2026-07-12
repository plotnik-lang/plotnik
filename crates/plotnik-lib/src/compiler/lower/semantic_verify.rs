//! Always-on verification at the executor fork point.
//!
//! This is the production trust boundary for lowered query IR. It mirrors the
//! bytecode validator's collecting effect-stack analysis, but works before any
//! target chooses a representation.

#[cfg(test)]
use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::bytecode::EffectKind;
use crate::compiler::analyze::output::{CaptureMemberKind, CaptureScopeKind, OutputSchema};
use crate::compiler::analyze::types::type_shape::{TYPE_VOID, TypeShape};
use crate::compiler::lower::ir::{
    EffectArg, EffectIR, InstructionIR, Label, MemberRef, NfaGraph, SemanticNfa,
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

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SemanticVerifyError {
    #[error("semantic matcher has {0} states (max {MAX_STATES})")]
    StateLimit(usize),
    #[error("semantic matcher has {0} entrypoints (max {max})", max = u16::MAX)]
    EntrypointLimit(usize),
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
    if graph.entrypoint_wrappers().len() > u16::MAX as usize {
        return Err(SemanticVerifyError::EntrypointLimit(
            graph.entrypoint_wrappers().len(),
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
            def_entries_consuming: IndexMap::new(),
            entrypoint_wrappers: IndexMap::new(),
            spans: None,
            label_origins: vec![None; count],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FrameKind {
    Array,
    Struct,
    Enum { is_void: bool, got_data: bool },
}

impl FrameKind {
    fn bit(self) -> u8 {
        match self {
            Self::Array => KS_ARRAY,
            Self::Struct => KS_STRUCT,
            Self::Enum { .. } => KS_ENUM,
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

const KS_ARRAY: u8 = 0b001;
const KS_STRUCT: u8 = 0b010;
const KS_ENUM: u8 = 0b100;
const KS_ANY: u8 = KS_ARRAY | KS_STRUCT | KS_ENUM;
const KS_SET: u8 = KS_STRUCT | KS_ENUM;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefSummary {
    entry_tos: u8,
    returns_pending: Option<bool>,
    sets_caller_top: bool,
}

impl DefSummary {
    fn unknown() -> Self {
        Self {
            entry_tos: KS_ANY,
            returns_pending: None,
            sets_caller_top: false,
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
            sets_caller_top: self.sets_caller_top | next.sets_caller_top,
        })
    }
}

type DefSummaries = HashMap<Label, DefSummary>;

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
        Ok(Self {
            graph,
            schema,
            instructions,
        })
    }

    fn verify_effects(&self) -> Result<(), SemanticVerifyError> {
        let wrappers: Vec<Label> = self.graph.entrypoint_wrappers().values().copied().collect();
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

            let analysis = self.analyze_body(&summaries, entry, called.contains(&entry), false)?;
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
                sets_caller_top: analysis.sets_caller_top,
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
            self.analyze_body(&summaries, entry, called.contains(&entry), true)?;
        }
        for entry in wrappers {
            let wrapper = self.analyze_body(&summaries, entry, called.contains(&entry), true)?;
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
        is_called: bool,
        final_check: bool,
    ) -> Result<BodyAnalysis, SemanticVerifyError> {
        #[cfg(test)]
        BODY_ANALYSES.set(BODY_ANALYSES.get() + 1);

        let mut entry_tos = KS_ANY;
        let mut returns_pending = None;
        let mut sets_caller_top = false;
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
                            EffectKind::ArrayOpen
                            | EffectKind::StructOpen
                            | EffectKind::EnumOpen => frame_openers += 1,
                            EffectKind::SuppressBegin => suppression_openers += 1,
                            EffectKind::SpanStartAt | EffectKind::SpanStart => span_openers += 1,
                            _ => {}
                        }
                    }
                }
                HashMap::new()
            });
            let state = match seen.entry(state) {
                Entry::Occupied(_) => continue,
                Entry::Vacant(entry) => {
                    let state = entry.key().clone();
                    entry.insert(());
                    state
                }
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
            let AbsState {
                mut stack,
                mut suppress,
                mut span_stack,
                mut pending,
            } = state;
            match instruction {
                InstructionIR::Return(_) => record_exit(
                    &stack,
                    suppress,
                    &span_stack,
                    pending,
                    &mut returns_pending,
                    label,
                )?,
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
                                sets_caller_top: &mut sets_caller_top,
                            },
                            label,
                        )?;
                    }
                    if matched.successors.is_empty() {
                        if is_called || !stack.is_empty() || suppress != 0 {
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
                        work.push((
                            call.next,
                            AbsState {
                                stack,
                                suppress,
                                span_stack,
                                pending,
                            },
                        ));
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
                        Some(kind) if final_check && kind.bit() & summary.entry_tos == 0 => {
                            return Err(SemanticVerifyError::EffectStack(label));
                        }
                        None => {
                            entry_tos &= summary.entry_tos;
                            sets_caller_top |= summary.sets_caller_top;
                        }
                        Some(_) => {}
                    }
                    let post_pending = summary
                        .returns_pending
                        .map(PendingState::from_bool)
                        .unwrap_or(PendingState::Unknown);
                    if summary.sets_caller_top
                        && let Some(FrameKind::Enum {
                            got_data: false, ..
                        }) = stack.last()
                    {
                        let mut written = stack.clone();
                        if let Some(FrameKind::Enum { got_data, .. }) = written.last_mut() {
                            *got_data = true;
                        }
                        work.push((
                            call.next,
                            AbsState {
                                stack: written,
                                suppress,
                                span_stack: span_stack.clone(),
                                pending: post_pending,
                            },
                        ));
                    }
                    work.push((
                        call.next,
                        AbsState {
                            stack,
                            suppress,
                            span_stack,
                            pending: post_pending,
                        },
                    ));
                }
            }
        }

        Ok(BodyAnalysis {
            entry_tos,
            returns_pending,
            sets_caller_top,
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
                _ => {}
            }
            return Ok(());
        }

        match effect.kind() {
            Node | Null => {
                if *state.pending == PendingState::Full {
                    return Err(SemanticVerifyError::EffectStack(label));
                }
                *state.pending = PendingState::Full;
            }
            SuppressBegin => *state.suppress += 1,
            SuppressEnd => return Err(SemanticVerifyError::EffectStack(label)),
            SpanStartAt | SpanStart => state.span_stack.push(literal(effect, label)?),
            SpanEnd => close_span(state.span_stack, literal(effect, label)?, label)?,
            ArrayOpen => open_frame(state.stack, state.pending, FrameKind::Array, label)?,
            StructOpen => open_frame(state.stack, state.pending, FrameKind::Struct, label)?,
            EnumOpen => {
                let member = member(effect, label)?;
                let is_void = self.enum_member_is_void(member, label)?;
                open_frame(
                    state.stack,
                    state.pending,
                    FrameKind::Enum {
                        is_void,
                        got_data: false,
                    },
                    label,
                )?;
            }
            Push => {
                require_pending(state.pending, label)?;
                match state.stack.last() {
                    Some(FrameKind::Array) => {}
                    Some(_) => return Err(SemanticVerifyError::EffectStack(label)),
                    None => *state.entry_tos &= KS_ARRAY,
                }
                *state.pending = PendingState::Empty;
            }
            Set => {
                self.validate_set_member(member(effect, label)?, label)?;
                require_pending(state.pending, label)?;
                match state.stack.last_mut() {
                    Some(FrameKind::Struct) => {}
                    Some(FrameKind::Enum { got_data, .. }) => *got_data = true,
                    Some(FrameKind::Array) => {
                        return Err(SemanticVerifyError::EffectStack(label));
                    }
                    None => {
                        *state.entry_tos &= KS_SET;
                        *state.sets_caller_top = true;
                    }
                }
                *state.pending = PendingState::Empty;
            }
            ArrayClose => close_simple_frame(state.stack, state.pending, FrameKind::Array, label)?,
            StructClose => {
                close_simple_frame(state.stack, state.pending, FrameKind::Struct, label)?
            }
            EnumClose => match state.stack.pop() {
                Some(FrameKind::Enum { is_void, got_data }) => {
                    let data_pending = match *state.pending {
                        PendingState::Full => true,
                        PendingState::Empty => false,
                        PendingState::Unknown => !got_data && !is_void,
                    };
                    if data_pending && got_data || (data_pending || got_data) == is_void {
                        return Err(SemanticVerifyError::EffectStack(label));
                    }
                    *state.pending = PendingState::Full;
                }
                _ => return Err(SemanticVerifyError::EffectStack(label)),
            },
        }
        Ok(())
    }

    fn enum_member_is_void(
        &self,
        member: MemberRef,
        label: Label,
    ) -> Result<bool, SemanticVerifyError> {
        let scope = self.member_scope(member, label)?;
        if scope.kind() != CaptureScopeKind::Enum {
            return Err(capture_error(
                label,
                "EnumOpen does not reference an enum variant",
            ));
        }
        let CaptureMemberKind::Variant(payload) =
            scope.members()[member.relative_index as usize].kind
        else {
            return Err(capture_error(
                label,
                "EnumOpen does not reference an enum variant",
            ));
        };
        Ok(payload == TYPE_VOID
            || matches!(
                self.schema.types.expect_type_shape(payload),
                TypeShape::Void
            ))
    }

    fn validate_set_member(
        &self,
        member: MemberRef,
        label: Label,
    ) -> Result<(), SemanticVerifyError> {
        let scope = self.member_scope(member, label)?;
        if scope.kind() == CaptureScopeKind::Struct
            && !matches!(
                scope.members()[member.relative_index as usize].kind,
                CaptureMemberKind::Field(_)
            )
        {
            return Err(capture_error(label, "Set references a non-field member"));
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
        for entry in self.entries() {
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
                        if matched.successors.is_empty() && next_depth != 0 {
                            return Err(SemanticVerifyError::CursorDepth(format!(
                                "accepting state {label:?} exits at depth {next_depth}"
                            )));
                        }
                        for successor in &matched.successors {
                            work.push((*successor, next_depth));
                        }
                    }
                    InstructionIR::Call(call) => {
                        work.push((call.next, depth + call.nav.depth_delta()));
                    }
                    InstructionIR::Return(_) if depth != 0 => {
                        return Err(SemanticVerifyError::CursorDepth(format!(
                            "return state {label:?} exits at depth {depth}"
                        )));
                    }
                    InstructionIR::Return(_) => {}
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
                            && matched.effects.iter().any(|effect| {
                                matches!(effect.kind(), EffectKind::Node | EffectKind::SpanStartAt)
                            })
                        {
                            return Err(SemanticVerifyError::ZeroWidthCursorRead(label));
                        }
                        for successor in &matched.successors {
                            work.push((*successor, after));
                        }
                    }
                    InstructionIR::Call(call) => work.push((call.next, false)),
                    InstructionIR::Return(_) => {}
                }
            }
        }
        Ok(())
    }

    fn entries(&self) -> impl Iterator<Item = Label> + '_ {
        self.graph
            .entrypoint_wrappers
            .values()
            .chain(self.graph.def_entries.values())
            .chain(self.graph.def_entries_consuming.values())
            .copied()
    }
}

struct BodyAnalysis {
    entry_tos: u8,
    returns_pending: Option<bool>,
    sets_caller_top: bool,
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
}

struct EffectState<'a> {
    stack: &'a mut Vec<FrameKind>,
    suppress: &'a mut usize,
    span_stack: &'a mut Vec<usize>,
    pending: &'a mut PendingState,
    entry_tos: &'a mut u8,
    sets_caller_top: &'a mut bool,
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

fn record_exit(
    stack: &[FrameKind],
    suppress: usize,
    spans: &[usize],
    pending: PendingState,
    returns_pending: &mut Option<bool>,
    label: Label,
) -> Result<(), SemanticVerifyError> {
    if !stack.is_empty() || suppress != 0 {
        return Err(SemanticVerifyError::EffectStack(label));
    }
    if !spans.is_empty() {
        return Err(SemanticVerifyError::SpanStack(label));
    }
    if let Some(pending) = pending.known() {
        if let Some(seen) = *returns_pending
            && seen != pending
        {
            return Err(SemanticVerifyError::EffectStack(label));
        }
        *returns_pending = Some(pending);
    }
    Ok(())
}

fn enqueue(queue: &mut VecDeque<Label>, queued: &mut HashSet<Label>, entry: Label) {
    if queued.insert(entry) {
        queue.push_back(entry);
    }
}
