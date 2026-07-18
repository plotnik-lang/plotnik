//! Target-neutral verification of matcher control flow and result effects.
//!
//! Both the semantic NFA and validated bytecode project into this small program
//! shape. Keeping the abstract interpreter here prevents their two trust
//! boundaries from maintaining parallel implementations of recursive call
//! summaries, materializer-stack safety, return routing, and cursor depth.
//!
//! Representation adapters resolve their own metadata before constructing a
//! [`Program`], so this layer needs only normalized instructions and body
//! contracts. It proves every control-flow path, not just every instruction:
//! deduplicated alternative tails may legitimately reach one address with
//! different builder stacks, so effect analysis keeps a set of abstract states
//! per address.
//!
//! Recursive bodies cannot be inlined into that analysis. A monotone fixpoint
//! instead summarizes which caller-top frames a body accepts, whether it writes
//! through that frame, and what pending-value state it returns. Derived opener
//! bounds reject net-growing cycles; a separate state budget bounds pathological
//! finite combinations.

use std::collections::hash_map::Entry as HashEntry;
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Debug;
use std::hash::Hash;

use plotnik_rt::PortId;

use crate::bytecode::{
    CalleeContract, EffectKind, EntryBoundary, FrameAction, Nav, ValueFrameKind,
};

const STATE_BUDGET: usize = 1 << 18;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Effect {
    kind: EffectKind,
    payload: usize,
    variant_has_no_payload: Option<bool>,
}

impl Effect {
    pub(crate) fn new(kind: EffectKind, payload: usize) -> Self {
        assert_ne!(
            kind,
            EffectKind::VariantOpen,
            "VariantOpen needs resolved payload metadata"
        );
        Self {
            kind,
            payload,
            variant_has_no_payload: None,
        }
    }

    pub(crate) fn variant_open(payload: usize, has_no_payload: bool) -> Self {
        Self {
            kind: EffectKind::VariantOpen,
            payload,
            variant_has_no_payload: Some(has_no_payload),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Match<A> {
    nav: Nav,
    effects: Box<[Effect]>,
    successors: Box<[A]>,
}

impl<A> Match<A> {
    pub(crate) fn new(
        nav: Nav,
        effects: impl Into<Box<[Effect]>>,
        successors: impl Into<Box<[A]>>,
    ) -> Self {
        Self {
            nav,
            effects: effects.into(),
            successors: successors.into(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Call<A> {
    nav: Nav,
    contract: CalleeContract,
    target: A,
    returns: Box<[A]>,
    consumed_mask: u8,
}

impl<A> Call<A> {
    pub(crate) fn new(
        nav: Nav,
        contract: CalleeContract,
        target: A,
        returns: impl Into<Box<[A]>>,
        consumed_mask: u8,
    ) -> Self {
        Self {
            nav,
            contract,
            target,
            returns: returns.into(),
            consumed_mask,
        }
    }

    fn body_contract(&self) -> BodyContract {
        BodyContract::new(self.contract, self.returns.len(), self.consumed_mask)
    }

    fn port_consumed(&self, port: PortId) -> bool {
        self.consumed_mask & port.bit() != 0
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Return {
    port: PortId,
    contract: CalleeContract,
}

impl Return {
    pub(crate) fn new(port: PortId, contract: CalleeContract) -> Self {
        Self { port, contract }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum Instruction<A> {
    Match(Match<A>),
    Call(Call<A>),
    Return(Return),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Entry<A> {
    target: A,
    boundary: EntryBoundary,
}

impl<A> Entry<A> {
    pub(crate) fn new(target: A, boundary: EntryBoundary) -> Self {
        Self { target, boundary }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BodyContract {
    contract: CalleeContract,
    arity: u8,
    consumed_mask: u8,
}

impl BodyContract {
    pub(crate) fn new(contract: CalleeContract, arity: usize, consumed_mask: u8) -> Self {
        Self {
            contract,
            arity: u8::try_from(arity).expect("matcher call arity fits u8"),
            consumed_mask,
        }
    }

    fn entry_point() -> Self {
        Self::new(CalleeContract::CallerOwned, 1, PortId::ZERO.bit())
    }

    fn validate<A: Copy>(&self, at: A) -> Result<(), VerifyError<A>> {
        if self.arity == 0 || self.arity > PortId::COUNT {
            return Err(VerifyError::malformed(
                Some(at),
                "body contract has an invalid return arity",
            ));
        }
        let dense = PortId::dense_mask(usize::from(self.arity));
        if self.consumed_mask & !dense != 0 {
            return Err(VerifyError::malformed(
                Some(at),
                "body contract consumes a port outside its arity",
            ));
        }
        if self.contract == CalleeContract::CallerOwned && self.consumed_mask != dense {
            return Err(VerifyError::malformed(
                Some(at),
                "caller-owned body exposes an empty return port",
            ));
        }
        Ok(())
    }

    fn expected_exit(self, port: PortId) -> Option<i32> {
        if port.index() >= usize::from(self.arity) {
            return None;
        }
        let consumed = self.consumed_mask & port.bit() != 0;
        match self.contract {
            CalleeContract::CallerOwned => consumed.then_some(0),
            CalleeContract::CalleeOwned { nav, .. } => {
                Some(if consumed { nav.depth_delta() } else { 0 })
            }
        }
    }
}

pub(crate) struct Program<A> {
    instructions: HashMap<A, Instruction<A>>,
    entries: Vec<Entry<A>>,
    roots: HashMap<A, BodyContract>,
}

impl<A> Program<A>
where
    A: Copy + Eq + Hash + Debug,
{
    pub(crate) fn new(
        instructions: impl IntoIterator<Item = (A, Instruction<A>)>,
        entries: Vec<Entry<A>>,
        declared_roots: impl IntoIterator<Item = (A, BodyContract)>,
    ) -> Result<Self, VerifyError<A>> {
        let mut instruction_map = HashMap::new();
        for (address, instruction) in instructions {
            if instruction_map.insert(address, instruction).is_some() {
                return Err(VerifyError::malformed(
                    Some(address),
                    "duplicate instruction address",
                ));
            }
        }

        let mut roots = HashMap::new();
        for (address, contract) in declared_roots {
            insert_root(&mut roots, address, contract)?;
        }
        for entry in &entries {
            insert_root(&mut roots, entry.target, BodyContract::entry_point())?;
        }
        for instruction in instruction_map.values() {
            if let Instruction::Call(call) = instruction {
                insert_root(&mut roots, call.target, call.body_contract())?;
            }
        }

        let program = Self {
            instructions: instruction_map,
            entries,
            roots,
        };
        program.validate_references()?;
        Ok(program)
    }

    fn validate_references(&self) -> Result<(), VerifyError<A>> {
        for entry in &self.entries {
            self.require_address(entry.target, None, "dangling entry target")?;
        }
        for &root in self.roots.keys() {
            self.require_address(root, Some(root), "dangling body root")?;
        }
        for (&address, instruction) in &self.instructions {
            match instruction {
                Instruction::Match(matched) => {
                    for &successor in &matched.successors {
                        self.require_address(successor, Some(address), "dangling match successor")?;
                    }
                }
                Instruction::Call(call) => {
                    self.require_address(call.target, Some(address), "dangling call target")?;
                    for &continuation in &call.returns {
                        self.require_address(
                            continuation,
                            Some(address),
                            "dangling call continuation",
                        )?;
                    }
                }
                Instruction::Return(_) => {}
            }
        }
        Ok(())
    }

    fn require_address(
        &self,
        target: A,
        from: Option<A>,
        detail: &'static str,
    ) -> Result<(), VerifyError<A>> {
        if self.instructions.contains_key(&target) {
            return Ok(());
        }
        Err(VerifyError::malformed(from.or(Some(target)), detail))
    }

    fn instruction(&self, address: A) -> &Instruction<A> {
        self.instructions
            .get(&address)
            .expect("program construction validates every control-flow address")
    }
}

fn insert_root<A>(
    roots: &mut HashMap<A, BodyContract>,
    address: A,
    contract: BodyContract,
) -> Result<(), VerifyError<A>>
where
    A: Copy + Eq + Hash,
{
    contract.validate(address)?;
    if let Some(previous) = roots.insert(address, contract)
        && previous != contract
    {
        return Err(VerifyError::malformed(
            Some(address),
            "one body root has conflicting call contracts",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VerifyError<A> {
    Malformed { at: Option<A>, detail: String },
    EffectStack(A),
    SpanStack(A),
    StateBudget(A),
    CursorDepth { at: A, detail: String },
    EmptyPathCursorRead(A),
}

impl<A> VerifyError<A> {
    fn malformed(at: Option<A>, detail: impl Into<String>) -> Self {
        Self::Malformed {
            at,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct VerifyStats {
    pub(crate) body_analyses: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EmptyPathCheck {
    Verify,
    Skip,
}

pub(crate) fn verify<A>(
    program: &Program<A>,
    empty_paths: EmptyPathCheck,
) -> Result<VerifyStats, VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    verify_return_routes(program)?;
    verify_cursor_depth(program)?;
    if empty_paths == EmptyPathCheck::Verify {
        verify_empty_paths(program)?;
    }
    verify_effects(program)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReturnContract {
    ports: u8,
    entry: Option<CalleeContract>,
    mixed_entries: bool,
}

impl ReturnContract {
    const NONE: Self = Self {
        ports: 0,
        entry: None,
        mixed_entries: false,
    };

    fn insert(&mut self, returned: Return) {
        self.ports |= returned.port.bit();
        match self.entry {
            None => self.entry = Some(returned.contract),
            Some(entry) if entry != returned.contract => self.mixed_entries = true,
            Some(_) => {}
        }
    }

    fn matches(self, expected: BodyContract) -> bool {
        self.ports == PortId::dense_mask(usize::from(expected.arity))
            && self.entry == Some(expected.contract)
            && !self.mixed_entries
    }
}

fn verify_return_routes<A>(program: &Program<A>) -> Result<(), VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    let mut cache = HashMap::new();
    for (&entry, &expected) in &program.roots {
        if !return_contract(program, entry, &mut cache).matches(expected) {
            return Err(VerifyError::malformed(
                Some(entry),
                "body has a malformed entry or return-port contract",
            ));
        }
    }
    Ok(())
}

fn return_contract<A>(
    program: &Program<A>,
    entry: A,
    cache: &mut HashMap<A, ReturnContract>,
) -> ReturnContract
where
    A: Copy + Eq + Hash + Debug,
{
    if let Some(&contract) = cache.get(&entry) {
        return contract;
    }

    let mut contract = ReturnContract::NONE;
    let mut seen = HashSet::new();
    let mut work = vec![entry];
    while let Some(address) = work.pop() {
        if !seen.insert(address) {
            continue;
        }
        match program.instruction(address) {
            Instruction::Match(matched) => work.extend(matched.successors.iter().copied()),
            Instruction::Call(call) => work.extend(call.returns.iter().copied()),
            Instruction::Return(returned) => contract.insert(*returned),
        }
    }
    cache.insert(entry, contract);
    contract
}

fn verify_cursor_depth<A>(program: &Program<A>) -> Result<(), VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    for (&entry, &route) in &program.roots {
        let mut memo = HashMap::new();
        let mut work = vec![(entry, 0i32)];
        while let Some((address, depth)) = work.pop() {
            if let Some(seen) = memo.insert(address, depth) {
                if seen != depth {
                    return Err(VerifyError::CursorDepth {
                        at: address,
                        detail: format!("state reached at depths {seen} and {depth}"),
                    });
                }
                continue;
            }

            match program.instruction(address) {
                Instruction::Match(matched) => {
                    let next_depth = depth + matched.nav.depth_delta();
                    if matched.successors.is_empty() {
                        let Some(expected) = route.expected_exit(PortId::ZERO) else {
                            return Err(depth_error(
                                address,
                                "accepting state has no declared zero port",
                            ));
                        };
                        if next_depth != expected {
                            return Err(VerifyError::CursorDepth {
                                at: address,
                                detail: format!(
                                    "accepting state exits at depth {next_depth}, expected {expected}"
                                ),
                            });
                        }
                        continue;
                    }
                    for &successor in &matched.successors {
                        work.push((successor, next_depth));
                    }
                }
                Instruction::Call(call) => {
                    for (index, &continuation) in call.returns.iter().enumerate() {
                        let port = PortId::new(index as u8)
                            .expect("matcher calls have at most eight ports");
                        let delta = if call.port_consumed(port) {
                            call.nav.depth_delta()
                        } else {
                            0
                        };
                        work.push((continuation, depth + delta));
                    }
                }
                Instruction::Return(returned) => {
                    let Some(expected) = route.expected_exit(returned.port) else {
                        return Err(depth_error(
                            address,
                            "return uses an undeclared or unsupported port",
                        ));
                    };
                    if depth != expected {
                        return Err(VerifyError::CursorDepth {
                            at: address,
                            detail: format!(
                                "return state exits at depth {depth}, expected {expected}"
                            ),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

fn depth_error<A>(at: A, detail: impl Into<String>) -> VerifyError<A> {
    VerifyError::CursorDepth {
        at,
        detail: detail.into(),
    }
}

fn verify_empty_paths<A>(program: &Program<A>) -> Result<(), VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    for entry in &program.entries {
        verify_empty_paths_from(program, entry.target, entry.boundary == EntryBoundary::Node)?;
    }
    for &entry in program.roots.keys() {
        verify_empty_paths_from(program, entry, false)?;
    }
    Ok(())
}

fn verify_empty_paths_from<A>(
    program: &Program<A>,
    entry: A,
    boundary_reads_cursor: bool,
) -> Result<(), VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    let mut memo = HashMap::new();
    let mut work = vec![(entry, true)];
    while let Some((address, empty_path)) = work.pop() {
        if let Some(seen_empty_path) = memo.get(&address)
            && (*seen_empty_path || !empty_path)
        {
            continue;
        }
        memo.insert(address, empty_path);

        match program.instruction(address) {
            Instruction::Match(matched) => {
                let after = empty_path && matched.nav == Nav::Epsilon;
                if after
                    && (boundary_reads_cursor && matched.successors.is_empty()
                        || matched
                            .effects
                            .iter()
                            .any(|effect| effect.kind.reads_cursor()))
                {
                    return Err(VerifyError::EmptyPathCursorRead(address));
                }
                for &successor in &matched.successors {
                    work.push((successor, after));
                }
            }
            Instruction::Call(call) => {
                for (index, &continuation) in call.returns.iter().enumerate() {
                    let port =
                        PortId::new(index as u8).expect("matcher calls have at most eight ports");
                    work.push((
                        continuation,
                        if call.port_consumed(port) {
                            false
                        } else {
                            empty_path
                        },
                    ));
                }
            }
            Instruction::Return(_) if boundary_reads_cursor && empty_path => {
                return Err(VerifyError::EmptyPathCursorRead(address));
            }
            Instruction::Return(_) => {}
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum FrameKind {
    List,
    Record,
    Variant {
        has_no_payload: bool,
        wrote_payload_fields: bool,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyRole {
    Root,
    Called,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VerifyPhase {
    Summarize,
    Final,
}

struct BodyAnalysis<A> {
    entry_tos: u8,
    returns_pending: Option<bool>,
    record_sets_caller_top: bool,
    discovered: Vec<A>,
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

    fn record_exit<A: Copy>(
        &self,
        returns_pending: &mut Option<bool>,
        address: A,
    ) -> Result<(), VerifyError<A>> {
        if !self.stack.is_empty() || self.suppress != 0 {
            return Err(VerifyError::EffectStack(address));
        }
        if !self.span_stack.is_empty() {
            return Err(VerifyError::SpanStack(address));
        }
        if let Some(pending) = self.pending.known() {
            if let Some(seen) = *returns_pending
                && seen != pending
            {
                return Err(VerifyError::EffectStack(address));
            }
            *returns_pending = Some(pending);
        }
        Ok(())
    }
}

fn verify_effects<A>(program: &Program<A>) -> Result<VerifyStats, VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    let mut definitions = Vec::new();
    let mut known = HashSet::new();
    let mut called = HashSet::new();
    let mut summaries = HashMap::new();
    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    let mut callers: HashMap<A, Vec<A>> = HashMap::new();
    let mut call_edges = HashSet::new();
    let mut stats = VerifyStats::default();

    for entry in &program.entries {
        if known.insert(entry.target) {
            definitions.push(entry.target);
            summaries.insert(entry.target, DefSummary::unknown());
            enqueue(&mut queue, &mut queued, entry.target);
        }
    }

    while let Some(entry) = queue.pop_front() {
        queued.remove(&entry);
        let role = if called.contains(&entry) {
            BodyRole::Called
        } else {
            BodyRole::Root
        };
        let analysis = analyze_body(
            program,
            &summaries,
            entry,
            role,
            VerifyPhase::Summarize,
            &mut stats,
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

        let next_summary = DefSummary {
            entry_tos: analysis.entry_tos,
            returns_pending: analysis.returns_pending,
            record_sets_caller_top: analysis.record_sets_caller_top,
        };
        let old = summaries[&entry];
        let Some(next) = old.refine(next_summary) else {
            return Err(VerifyError::EffectStack(entry));
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
        let role = if called.contains(&entry) {
            BodyRole::Called
        } else {
            BodyRole::Root
        };
        analyze_body(
            program,
            &summaries,
            entry,
            role,
            VerifyPhase::Final,
            &mut stats,
        )?;
    }

    for entry in &program.entries {
        let summary = summaries[&entry.target];
        let accepts_caller_top = match entry.boundary {
            EntryBoundary::Record => summary.entry_tos & KS_RECORD != 0,
            EntryBoundary::Passthrough | EntryBoundary::Node => summary.entry_tos == KS_ANY,
        };
        if !accepts_caller_top
            || matches!(entry.boundary, EntryBoundary::Node | EntryBoundary::Record)
                && summary.returns_pending == Some(true)
        {
            return Err(VerifyError::EffectStack(entry.target));
        }
    }

    Ok(stats)
}

fn analyze_body<A>(
    program: &Program<A>,
    summaries: &HashMap<A, DefSummary>,
    entry: A,
    role: BodyRole,
    phase: VerifyPhase,
    stats: &mut VerifyStats,
) -> Result<BodyAnalysis<A>, VerifyError<A>>
where
    A: Copy + Eq + Hash + Debug,
{
    stats.body_analyses += 1;

    let mut entry_tos = KS_ANY;
    let mut returns_pending = None;
    let mut record_sets_caller_top = false;
    let mut discovered = Vec::new();
    let mut discovered_set = HashSet::new();
    let mut memo: HashMap<A, HashMap<AbsState, ()>> = HashMap::new();
    let mut states_spent = 0;
    let mut frame_openers = 0;
    let mut suppression_openers = 0;
    let mut span_openers = 0;
    let mut work = vec![(entry, AbsState::initial())];

    while let Some((address, state)) = work.pop() {
        let instruction = program.instruction(address);
        let seen = memo.entry(address).or_insert_with(|| {
            if let Instruction::Match(matched) = instruction {
                for effect in &matched.effects {
                    match effect.kind {
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
            return Err(VerifyError::EffectStack(address));
        }
        if state.span_stack.len() > span_openers {
            return Err(VerifyError::SpanStack(address));
        }
        states_spent += 1;
        if states_spent > STATE_BUDGET {
            return Err(VerifyError::StateBudget(address));
        }
        if matches!(instruction, Instruction::Return(_)) {
            state.record_exit(&mut returns_pending, address)?;
            continue;
        }

        let AbsState {
            mut stack,
            mut suppress,
            mut span_stack,
            mut pending,
        } = state;

        match instruction {
            Instruction::Return(_) => unreachable!("returns exit before state mutation"),
            Instruction::Match(matched) => {
                for &effect in &matched.effects {
                    apply_effect(
                        effect,
                        EffectState {
                            stack: &mut stack,
                            suppress: &mut suppress,
                            span_stack: &mut span_stack,
                            pending: &mut pending,
                            entry_tos: &mut entry_tos,
                            record_sets_caller_top: &mut record_sets_caller_top,
                        },
                        address,
                    )?;
                }
                if matched.successors.is_empty() {
                    if role == BodyRole::Called || !stack.is_empty() || suppress != 0 {
                        return Err(VerifyError::EffectStack(address));
                    }
                    if !span_stack.is_empty() {
                        return Err(VerifyError::SpanStack(address));
                    }
                    continue;
                }
                for &successor in &matched.successors {
                    work.push((
                        successor,
                        AbsState {
                            stack: stack.clone(),
                            suppress,
                            span_stack: span_stack.clone(),
                            pending,
                        },
                    ));
                }
            }
            Instruction::Call(call) => {
                if discovered_set.insert(call.target) {
                    discovered.push(call.target);
                }
                if suppress > 0 {
                    push_call_returns(
                        &mut work,
                        call,
                        AbsState {
                            stack,
                            suppress,
                            span_stack,
                            pending,
                        },
                    );
                    continue;
                }
                if pending == PendingState::Full {
                    return Err(VerifyError::EffectStack(address));
                }

                let summary = match phase {
                    VerifyPhase::Summarize => summaries
                        .get(&call.target)
                        .copied()
                        .unwrap_or_else(DefSummary::unknown),
                    VerifyPhase::Final => *summaries
                        .get(&call.target)
                        .expect("summary walk discovers every reachable call target"),
                };
                match stack.last() {
                    Some(kind)
                        if phase == VerifyPhase::Final && kind.bit() & summary.entry_tos == 0 =>
                    {
                        return Err(VerifyError::EffectStack(address));
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
                        wrote_payload_fields: false,
                        ..
                    }) = stack.last()
                {
                    let mut written = stack.clone();
                    if let Some(FrameKind::Variant {
                        wrote_payload_fields,
                        ..
                    }) = written.last_mut()
                    {
                        *wrote_payload_fields = true;
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

fn take_unseen_state(seen: &mut HashMap<AbsState, ()>, state: AbsState) -> Option<AbsState> {
    match seen.entry(state) {
        HashEntry::Occupied(_) => None,
        HashEntry::Vacant(entry) => {
            let state = entry.key().clone();
            entry.insert(());
            Some(state)
        }
    }
}

fn push_call_returns<A: Copy>(work: &mut Vec<(A, AbsState)>, call: &Call<A>, state: AbsState) {
    for &continuation in call.returns.iter().rev() {
        work.push((continuation, state.clone()));
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

fn apply_effect<A: Copy>(
    effect: Effect,
    state: EffectState<'_>,
    address: A,
) -> Result<(), VerifyError<A>> {
    use EffectKind::*;

    if *state.suppress > 0 {
        match effect.kind {
            SuppressBegin => *state.suppress += 1,
            SuppressEnd => *state.suppress -= 1,
            SpanStartAt | SpanStart => state.span_stack.push(effect.payload),
            SpanEnd => close_span(state.span_stack, effect.payload, address)?,
            ScalarMark => {}
            _ => {}
        }
        return Ok(());
    }

    if effect.kind.frame_action().is_some() {
        return apply_frame_action(effect, state, address);
    }

    match effect.kind {
        Node | Absent | NodeText | NodeBool | BoolValue => {
            if *state.pending == PendingState::Full {
                return Err(VerifyError::EffectStack(address));
            }
            *state.pending = PendingState::Full;
        }
        SuppressBegin => *state.suppress += 1,
        SuppressEnd => return Err(VerifyError::EffectStack(address)),
        SpanStartAt | SpanStart => state.span_stack.push(effect.payload),
        SpanEnd => close_span(state.span_stack, effect.payload, address)?,
        ScalarMark => {}
        ArrayPush => {
            require_pending(state.pending, address)?;
            match state.stack.last() {
                Some(FrameKind::List) => {}
                Some(_) => return Err(VerifyError::EffectStack(address)),
                None => *state.entry_tos &= KS_LIST,
            }
            *state.pending = PendingState::Empty;
        }
        RecordSet => {
            require_pending(state.pending, address)?;
            match state.stack.last_mut() {
                Some(FrameKind::Record) => {}
                Some(FrameKind::Variant {
                    wrote_payload_fields,
                    ..
                }) => *wrote_payload_fields = true,
                Some(FrameKind::List | FrameKind::Scalar) => {
                    return Err(VerifyError::EffectStack(address));
                }
                None => {
                    *state.entry_tos &= KS_RECORD_SET_TARGET;
                    *state.record_sets_caller_top = true;
                }
            }
            *state.pending = PendingState::Empty;
        }
        ListOpen | ListClose | RecordOpen | RecordClose | VariantOpen | VariantClose
        | ScalarOpen | TextClose | BoolClose => {
            unreachable!("frame effects return before data dispatch")
        }
    }
    Ok(())
}

fn apply_frame_action<A: Copy>(
    effect: Effect,
    state: EffectState<'_>,
    address: A,
) -> Result<(), VerifyError<A>> {
    let action = effect
        .kind
        .frame_action()
        .expect("frame action effects are dispatched here");
    match action {
        FrameAction::Open(kind) => {
            let frame = match kind {
                ValueFrameKind::List => FrameKind::List,
                ValueFrameKind::Record => FrameKind::Record,
                ValueFrameKind::Variant => FrameKind::Variant {
                    has_no_payload: effect
                        .variant_has_no_payload
                        .expect("VariantOpen metadata is resolved by the program adapter"),
                    wrote_payload_fields: false,
                },
                ValueFrameKind::Scalar => FrameKind::Scalar,
            };
            open_frame(state.stack, state.pending, frame, address)?;
        }
        FrameAction::Close(ValueFrameKind::Variant) => match state.stack.pop() {
            Some(FrameKind::Variant {
                has_no_payload,
                wrote_payload_fields,
            }) => {
                let payload_pending = match *state.pending {
                    PendingState::Full => true,
                    PendingState::Empty => false,
                    PendingState::Unknown => !wrote_payload_fields && !has_no_payload,
                };
                if payload_pending && wrote_payload_fields
                    || (payload_pending || wrote_payload_fields) == has_no_payload
                {
                    return Err(VerifyError::EffectStack(address));
                }
                *state.pending = PendingState::Full;
            }
            _ => return Err(VerifyError::EffectStack(address)),
        },
        FrameAction::Close(kind) => {
            let expected = match kind {
                ValueFrameKind::List => FrameKind::List,
                ValueFrameKind::Record => FrameKind::Record,
                ValueFrameKind::Scalar => FrameKind::Scalar,
                ValueFrameKind::Variant => unreachable!("variant close handled above"),
            };
            close_simple_frame(state.stack, state.pending, expected, address)?;
        }
    }
    Ok(())
}

fn open_frame<A: Copy>(
    stack: &mut Vec<FrameKind>,
    pending: &mut PendingState,
    frame: FrameKind,
    address: A,
) -> Result<(), VerifyError<A>> {
    if *pending == PendingState::Full {
        return Err(VerifyError::EffectStack(address));
    }
    stack.push(frame);
    Ok(())
}

fn require_pending<A: Copy>(pending: &PendingState, address: A) -> Result<(), VerifyError<A>> {
    if *pending == PendingState::Empty {
        return Err(VerifyError::EffectStack(address));
    }
    Ok(())
}

fn close_simple_frame<A: Copy>(
    stack: &mut Vec<FrameKind>,
    pending: &mut PendingState,
    expected: FrameKind,
    address: A,
) -> Result<(), VerifyError<A>> {
    if stack.pop() != Some(expected) || *pending == PendingState::Full {
        return Err(VerifyError::EffectStack(address));
    }
    *pending = PendingState::Full;
    Ok(())
}

fn close_span<A: Copy>(
    stack: &mut Vec<usize>,
    id: usize,
    address: A,
) -> Result<(), VerifyError<A>> {
    if stack.pop() != Some(id) {
        return Err(VerifyError::SpanStack(address));
    }
    Ok(())
}

fn enqueue<A: Copy + Eq + Hash>(queue: &mut VecDeque<A>, queued: &mut HashSet<A>, entry: A) {
    if queued.insert(entry) {
        queue.push_back(entry);
    }
}
