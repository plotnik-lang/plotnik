//! Interprocedural effect-stack verifier, run at load.
//!
//! The runtime `ValueMaterializer` is a stack machine over the flat effect
//! sequence of the winning path. Five of its operations panic on an ill-shaped
//! builder stack — `ArrayPush`/`ListClose` want a `List` on top, `RecordSet` a
//! `Record` or `Variant`, `RecordClose` a `Record`, `VariantClose` a `Variant`
//! (`crates/plotnik-lib/src/vm/engine/materializer.rs`) — plus `VariantClose`
//! panics when a payload arrives both as the pending value and as direct
//! fields, the end-of-log assert panics on unclosed frames, and the VM's
//! `emit_effect` panics if a `SuppressEnd` underflows the suppression counter
//! (`crates/plotnik-lib/src/vm/engine/vm.rs`). On valid compiler output these are
//! unreachable by construction; malformed output could still violate them.
//! This pass proves them unreachable for every module that passes
//! module validation, so they stay sound loud invariants instead of reachable
//! panics.
//!
//! ## Model
//!
//! The materializer's input is the inline concatenation of every committed
//! `Match`'s effects across `Call`/`Return` boundaries, with the VM's suppression
//! filter applied: `SuppressBegin`/`SuppressEnd` adjust a counter and, while it
//! is positive, every data effect is dropped before the log. So the abstract
//! state is `(stack, suppress, span_stack, pending)`: a builder-frame stack, a
//! suppression depth, the stack of open inspection span ids, and whether the
//! materializer's pending-value register is full. Tracking span *ids* (not just
//! a depth) proves every `SpanEnd` closes the span the matching bracket opened,
//! so inspection extraction can assert pairing instead of re-validating. The
//! walk starts from each entry point wrapper and follows `Match` successors,
//! descending through `Call` and resuming at its return address — exactly the
//! edge set that orders effects at runtime.
//!
//! ## Why state *sets* (collecting semantics)
//!
//! The panic-freedom property is per *path*: every graph path must execute to a
//! well-shaped effect sequence. Distinct paths may legally reach the same instruction
//! with different abstract states — the dedup pass hash-conses structurally
//! identical alternative tails, so e.g. two alternatives share one `VariantClose`
//! instruction, reached once under `Variant(A)` and once under `Variant(B)`. Each arrival
//! is individually sound (dedup is a bisimulation quotient: it preserves the
//! op-labeled path set exactly), so the walk keeps a *set* of states per instruction
//! and verifies every effect against every state that reaches it. Requiring a
//! single state per instruction — as this pass once did — rejected modules the
//! compiler itself emitted.
//!
//! Termination cannot rely on the state sets converging on their own — malformed
//! bytecode with a net-growing cycle would mint a deeper stack every lap. Two
//! bounds close that off, both loose enough that compiler output never trips
//! them:
//!
//! - **Derived depth bounds.** A state's frame stack can never legitimately be
//!   deeper than the total number of frame-opening effects on the instructions the
//!   walk has visited: every push comes from a visited instruction, and revisiting
//!   one at a strictly greater depth proves a net-positive cycle, which can
//!   never rebalance — every exit requires an empty local stack, so some path
//!   through such a cycle is provably ill-formed. The suppression counter and
//!   span stack get the same treatment against their own opener counts.
//! - **A state budget.** Frame payloads (variant member, `wrote_payload_fields`, pending) are
//!   finite but can multiply across nesting; a hard cap on states explored per
//!   body bounds load time on pathological compiler output. Valid output stays
//!   far below it: the states at a merged instruction correspond to the pre-dedup
//!   twins, and code addresses are `u16`, so a body contributes at most tens of
//!   thousands of states in total.
//!
//! ## Why summaries
//!
//! Inlining does not terminate: captured recursion grows the builder stack one
//! frame per level, opaque recursion grows the suppression counter. But every
//! definition body is, by the compiler's scope discipline, *net-neutral* on the
//! builder stack (it closes every frame it opens) and reads at most the caller's
//! top frame before pushing one of its own. So a body's whole interprocedural
//! effect collapses to a small summary — the set of caller-top kinds it
//! tolerates (`entry_tos`), whether it may `RecordSet` into the caller's top frame
//! (`record_sets_caller_top`), and the pending state it returns — plus the verified
//! facts that it is net-neutral and suppression-balanced. Calls apply that
//! summary instead of inlining, which both terminates and stays sound. The
//! summaries are computed by a monotone fixpoint (a callee that reads its
//! caller's top before pushing propagates the constraint up to its own
//! callers), then a final pass checks every call site and every entry point
//! wrapper against the stabilized summaries.
//!
//! `record_sets_caller_top` exists because a below-entry `RecordSet` mutates state the
//! caller's walk otherwise cannot see: setting a field on the caller's *variant*
//! frame gives the frame a payload at its `VariantClose`. A call site
//! whose local top is a variant therefore forks the state — one path assumes
//! the write happened, one that it did not — so stale payload-field state can never
//! smuggle a "payload arrived both as pending value and as direct fields"
//! panic past the check.
//!
//! A successor-less `Match` accepts the *whole run* from any call depth,
//! freezing the log with every caller frame still open. Inside an entry point
//! wrapper the local stack is the global stack, so the existing exit check is
//! exact; inside a body reachable through `Call` the caller's frames are
//! invisible here, so such accepts are rejected outright — the compiler ends
//! every definition body with `Return` and only accepts at wrapper level.
//!
//! Net-neutrality, no popping below entry, and suppression balance are not
//! assumed — they are verified, so a malformed body is rejected
//! rather than silently mismodeled.
//!
//! ## Scope
//!
//! This pass proves the release-build panic surface unreachable, plus the
//! span-pairing and variant payload-consistency assertions. Full agreement
//! between materialized values and the declared type tables (checked by
//! `debug_verify_type` in debug builds) is a compiler self-check, not a load
//! guarantee: malformed bytecode can still declare types its effects do not
//! produce.

#[cfg(test)]
use std::cell::Cell;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet, VecDeque};

use super::{Instruction, Module, ModuleError};
use crate::bytecode::{
    CodeAddr, Effect, EffectKind, FrameAction, TypeDefKind, TypeKind, ValueFrameKind,
};

/// Builder frames the materializer pushes. The root/result frame can be a
/// scalar, but compiled effects only push these three frame kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum FrameKind {
    List,
    Record,
    Variant {
        member: u16,
        wrote_payload_fields: bool,
    },
    Scalar,
}

impl FrameKind {
    fn bit(self) -> u8 {
        match self {
            FrameKind::List => KS_LIST,
            FrameKind::Record => KS_RECORD,
            FrameKind::Variant { .. } => KS_VARIANT,
            FrameKind::Scalar => KS_SCALAR,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum PendingState {
    Empty,
    Full,
    /// Produced by calls whose callee summary has not converged (recursion
    /// mid-fixpoint) or never converges. The latter means no exit of the
    /// callee ever has a known pending — every exit sits behind an unresolved
    /// recursive call — so the callee can never actually return at runtime
    /// and the states downstream of the call are unreachable; validating them
    /// permissively is sound.
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

// `entry_tos` is a set of tolerated caller-top kinds, a 3-bit mask.
const KS_LIST: u8 = 0b001;
const KS_RECORD: u8 = 0b010;
const KS_VARIANT: u8 = 0b100;
const KS_SCALAR: u8 = 0b1000;
/// No constraint (every kind tolerated): the body never reads its caller's top.
const KS_ANY: u8 = KS_LIST | KS_RECORD | KS_VARIANT | KS_SCALAR;
/// `RecordSet` targets — a `Record` or a `Variant` frame.
const KS_RECORD_SET_TARGET: u8 = KS_RECORD | KS_VARIANT;

/// Hard cap on abstract states explored per body walk. Valid output is bounded
/// by the pre-dedup instruction count (`u16` address space); pathological compiler
/// output is rejected before a combinatorial frame-payload blowup can monopolize
/// load time.
const STATE_BUDGET: usize = 1 << 18;

#[cfg(test)]
thread_local! {
    static BODY_ANALYSES: Cell<usize> = const { Cell::new(0) };
}

#[cfg(test)]
pub(super) fn reset_body_analyses() {
    BODY_ANALYSES.set(0);
}

#[cfg(test)]
pub(super) fn body_analyses() -> usize {
    BODY_ANALYSES.get()
}

#[cfg(test)]
fn record_body_analysis() {
    BODY_ANALYSES.set(BODY_ANALYSES.get() + 1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefSummary {
    entry_tos: u8,
    returns_pending: Option<bool>,
    /// Some path in the body applies `RecordSet` to the caller's top frame (directly or via a
    /// transitive callee). Call sites must account for the write both having
    /// and not having happened.
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

/// Summaries keyed by definition-entry address.
type DefSummaries = HashMap<CodeAddr, DefSummary>;

/// Verify that no path can drive the materializer or the suppression counter
/// into a panic. Assumes [`Module::validate_instructions`] has already run, so
/// every instruction decode and every jump target is safe.
pub(crate) fn validate_effect_stack(module: &Module) -> Result<(), ModuleError> {
    let entry_points = module.entry_points();

    let mut defs = Vec::new();
    let mut known = HashSet::new();
    let mut summaries = DefSummaries::new();
    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    for entry_point in entry_points.iter() {
        let target = entry_point.target();
        if known.insert(target) {
            defs.push(target);
            summaries.insert(target, DefSummary::unknown());
            enqueue(&mut queue, &mut queued, target);
        }
    }
    // Entries reached through a `Call` — these run with caller frames live, so
    // a whole-run accept inside them is banned (see module docs).
    let mut called = HashSet::new();
    // A summary change only invalidates direct callers. Keeping reverse edges
    // avoids rescanning every definition for every link in a dependency chain.
    let mut callers: HashMap<CodeAddr, Vec<CodeAddr>> = HashMap::new();
    let mut call_edges = HashSet::new();

    // Monotone fixpoint: `entry_tos` only ever shrinks (intersection),
    // `record_sets_caller_top` only flips on, `returns_pending` only becomes known,
    // and the definition/called sets only grow. FIFO scheduling coalesces a
    // batch of callee changes before a wide caller is revisited.
    while let Some(entry) = queue.pop_front() {
        queued.remove(&entry);

        let analysis = analyze(
            module,
            &summaries,
            entry,
            BodyRole::from_called(called.contains(&entry)),
            VerifyPhase::Summarize,
        )?;
        for target in analysis.discovered {
            if known.insert(target) {
                defs.push(target);
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
            return Err(ModuleError::EffectStackImbalance(entry));
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

    // Final pass: with stabilized constraints, check every call site (membership
    // of the caller's top in the callee's `entry_tos`) inside each body...
    for &entry in &defs {
        analyze(
            module,
            &summaries,
            entry,
            BodyRole::from_called(called.contains(&entry)),
            VerifyPhase::Final,
        )?;
    }

    // ...and every entry point wrapper. A wrapper has no caller, so a residual
    // caller-top constraint means some effect would read below the frames the
    // wrapper itself opened and hit the materializer's result root frame.
    for entry_point in entry_points.iter() {
        let target = entry_point.target();
        let wrapper = analyze(
            module,
            &summaries,
            target,
            BodyRole::from_called(called.contains(&target)),
            VerifyPhase::Final,
        )?;
        if wrapper.entry_tos != KS_ANY {
            return Err(ModuleError::EffectStackImbalance(target));
        }
    }

    Ok(())
}

fn enqueue(queue: &mut VecDeque<CodeAddr>, queued: &mut HashSet<CodeAddr>, entry: CodeAddr) {
    if queued.insert(entry) {
        queue.push_back(entry);
    }
}

struct Analysis {
    /// Caller-top kinds this body tolerates (intersection of every read).
    entry_tos: u8,
    /// Whether every exit from this body leaves a pending value.
    returns_pending: Option<bool>,
    /// Whether some path applies `RecordSet` to the caller's top frame.
    record_sets_caller_top: bool,
    /// Call targets reached — definitions to summarize.
    discovered: Vec<CodeAddr>,
}

/// Abstract state at instruction entry: the builder frames pushed so far
/// (relative to this body's entry), suppression depth, open span ids, and
/// pending register.
#[derive(Clone, PartialEq, Eq, Hash)]
struct AbsState {
    stack: Vec<FrameKind>,
    suppress: i32,
    span_stack: Vec<u16>,
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
        addr: CodeAddr,
    ) -> Result<(), ModuleError> {
        if !self.stack.is_empty() || self.suppress != 0 {
            return Err(ModuleError::EffectStackImbalance(addr));
        }
        if !self.span_stack.is_empty() {
            return Err(ModuleError::SpanImbalance(addr));
        }
        if let Some(pending) = self.pending.known() {
            if let Some(seen) = *returns_pending
                && seen != pending
            {
                return Err(ModuleError::EffectStackImbalance(addr));
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

/// Walk one body, computing its summary facts and verifying its structural
/// invariants against every abstract state that reaches each instruction. In
/// the final phase, also verify each call site against `summaries`.
fn analyze(
    module: &Module,
    summaries: &DefSummaries,
    entry: CodeAddr,
    role: BodyRole,
    phase: VerifyPhase,
) -> Result<Analysis, ModuleError> {
    #[cfg(test)]
    record_body_analysis();

    let mut entry_tos = KS_ANY;
    let mut returns_pending = None;
    let mut record_sets_caller_top = false;
    let mut discovered = Vec::new();
    let mut discovered_set = HashSet::new();

    // Collecting semantics: every distinct abstract state an instruction is reached
    // with is kept and processed once. The opener tallies accumulate, per
    // visited instruction, how many frame/suppression/span openers exist — a state
    // outgrowing them proves a net-positive cycle (see module docs).
    let mut memo: HashMap<CodeAddr, HashMap<AbsState, ()>> = HashMap::new();
    let mut states_spent: usize = 0;
    let mut frame_openers: usize = 0;
    let mut suppress_openers: i32 = 0;
    let mut span_openers: usize = 0;

    let mut work: Vec<(CodeAddr, AbsState)> = vec![(entry, AbsState::initial())];

    while let Some((addr, state)) = work.pop() {
        let instruction = module.decode_instruction(addr);

        let seen = memo.entry(addr).or_insert_with(|| {
            if let Instruction::Match(m) = &instruction {
                for eff in m.effects() {
                    match eff.kind {
                        EffectKind::ListOpen
                        | EffectKind::RecordOpen
                        | EffectKind::VariantOpen
                        | EffectKind::ScalarOpen => {
                            frame_openers += 1;
                        }
                        EffectKind::SuppressBegin => suppress_openers += 1,
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
        if state.stack.len() > frame_openers || state.suppress > suppress_openers {
            return Err(ModuleError::EffectStackImbalance(addr));
        }
        if state.span_stack.len() > span_openers {
            return Err(ModuleError::SpanImbalance(addr));
        }
        states_spent += 1;
        if states_spent > STATE_BUDGET {
            return Err(ModuleError::EffectStackBudget(addr));
        }
        if matches!(instruction, Instruction::Return(_)) {
            state.record_exit(&mut returns_pending, addr)?;
            continue;
        }
        let AbsState {
            mut stack,
            mut suppress,
            mut span_stack,
            mut pending,
        } = state;

        if let Some(call) = CallRoute::from_instruction(instruction) {
            if discovered_set.insert(call.target) {
                discovered.push(call.target);
            }

            if suppress > 0 {
                // A suppressed callee is frozen: all its output effects are
                // dropped, so it is a no-op on the builder stack.
                call.push_returns(
                    &mut work,
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
                return Err(ModuleError::EffectStackImbalance(addr));
            }

            let summary = summaries
                .get(&call.target)
                .copied()
                .unwrap_or_else(DefSummary::unknown);
            match stack.last() {
                Some(&kind)
                    if phase == VerifyPhase::Final && kind.bit() & summary.entry_tos == 0 =>
                {
                    return Err(ModuleError::EffectStackImbalance(addr));
                }
                None => {
                    // The callee's reads and writes land on *our* caller's top
                    // frame: inherit the constraint and the write flag.
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
                call.push_returns(
                    &mut work,
                    AbsState {
                        stack: written,
                        suppress,
                        span_stack: span_stack.clone(),
                        pending: post_pending,
                    },
                );
            }

            call.push_returns(
                &mut work,
                AbsState {
                    stack,
                    suppress,
                    span_stack,
                    pending: post_pending,
                },
            );
            continue;
        }

        match instruction {
            Instruction::Return(_) => unreachable!("returns exit before state mutation"),
            Instruction::Match(m) => {
                for eff in m.effects() {
                    apply_effect(
                        module,
                        eff,
                        EffectState {
                            stack: &mut stack,
                            suppress: &mut suppress,
                            span_stack: &mut span_stack,
                            pending: &mut pending,
                            entry_tos: &mut entry_tos,
                            record_sets_caller_top: &mut record_sets_caller_top,
                        },
                        addr,
                    )?;
                }
                if m.succ_count() == 0 {
                    // A successor-less match accepts the whole run. At wrapper
                    // level the local stack is the global stack, so balance
                    // here is exact; under a `Call` the caller's frames are
                    // still open in the log, so this is never sound.
                    if role == BodyRole::Called {
                        return Err(ModuleError::EffectStackImbalance(addr));
                    }
                    if !stack.is_empty() || suppress != 0 {
                        return Err(ModuleError::EffectStackImbalance(addr));
                    }
                    if !span_stack.is_empty() {
                        return Err(ModuleError::SpanImbalance(addr));
                    }
                } else {
                    for succ in m.successors() {
                        work.push((
                            CodeAddr::from(u16::from(succ)),
                            AbsState {
                                stack: stack.clone(),
                                suppress,
                                span_stack: span_stack.clone(),
                                pending,
                            },
                        ));
                    }
                }
            }
            Instruction::Call(_) | Instruction::RoutedCall(_) | Instruction::SplitCall(_) => {
                unreachable!("calls exit before ordinary instruction dispatch")
            }
        }
    }

    Ok(Analysis {
        entry_tos,
        returns_pending,
        record_sets_caller_top,
        discovered,
    })
}

#[derive(Clone, Copy)]
struct CallRoute {
    target: CodeAddr,
    returns: CallReturnAddrs,
}

impl CallRoute {
    fn from_instruction(instruction: Instruction<'_>) -> Option<Self> {
        match instruction {
            Instruction::Call(call) => Some(Self {
                target: CodeAddr::from(u16::from(call.target)),
                returns: CallReturnAddrs::Single([CodeAddr::from(u16::from(call.next))]),
            }),
            Instruction::RoutedCall(call) => Some(Self {
                target: CodeAddr::from(u16::from(call.target)),
                returns: CallReturnAddrs::Single([CodeAddr::from(u16::from(call.next))]),
            }),
            Instruction::SplitCall(call) => Some(Self {
                target: CodeAddr::from(u16::from(call.target)),
                returns: CallReturnAddrs::Split([
                    CodeAddr::from(u16::from(call.returns.matched)),
                    CodeAddr::from(u16::from(call.returns.empty)),
                ]),
            }),
            Instruction::Match(_) | Instruction::Return(_) => None,
        }
    }

    fn push_returns(self, work: &mut Vec<(CodeAddr, AbsState)>, state: AbsState) {
        match self.returns {
            CallReturnAddrs::Single([return_]) => work.push((return_, state)),
            CallReturnAddrs::Split([matched, empty]) => {
                work.push((empty, state.clone()));
                work.push((matched, state));
            }
        }
    }
}

#[derive(Clone, Copy)]
enum CallReturnAddrs {
    Single([CodeAddr; 1]),
    Split([CodeAddr; 2]),
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

/// Apply one effect to the abstract state. Records a caller-top constraint into
/// `entry_tos` when a read happens with no own frame on top, and rejects a
/// frame-kind mismatch, a pop below entry, or a suppression underflow.
fn apply_effect(
    module: &Module,
    effect: Effect,
    state: EffectState<'_>,
    addr: CodeAddr,
) -> Result<(), ModuleError> {
    use EffectKind::*;

    // Suppression and span brackets act regardless of depth; output effects are
    // dropped by the VM while suppressed and so must not touch the builder stack.
    if *state.suppress > 0 {
        match effect.kind {
            SuppressBegin => *state.suppress += 1,
            SuppressEnd => *state.suppress -= 1,
            SpanStartAt | SpanStart => state.span_stack.push(effect.payload as u16),
            SpanEnd => close_span(state.span_stack, effect.payload as u16, addr)?,
            ScalarMark => {}
            _ => {}
        }
        return Ok(());
    }

    if effect.kind.frame_action().is_some() {
        return apply_frame_action(module, effect, state, addr);
    }

    let err = || ModuleError::EffectStackImbalance(addr);
    match effect.kind {
        Node | Absent | NodeStr | NodeBool | BoolValue => {
            if *state.pending == PendingState::Full {
                return Err(err());
            }
            *state.pending = PendingState::Full;
        }
        SuppressBegin => *state.suppress += 1,
        // At depth 0 a `SuppressEnd` would drive the counter negative — the
        // exact underflow the VM panics on.
        SuppressEnd => return Err(err()),
        SpanStartAt | SpanStart => state.span_stack.push(effect.payload as u16),
        SpanEnd => close_span(state.span_stack, effect.payload as u16, addr)?,
        ScalarMark => {}
        ArrayPush => {
            if *state.pending == PendingState::Empty {
                return Err(err());
            }
            match state.stack.last() {
                Some(FrameKind::List) => {}
                Some(_) => return Err(err()),
                None => *state.entry_tos &= KS_LIST,
            }
            *state.pending = PendingState::Empty;
        }
        RecordSet => {
            if *state.pending == PendingState::Empty {
                return Err(err());
            }
            match state.stack.last_mut() {
                Some(FrameKind::Record) => {}
                Some(FrameKind::Variant {
                    wrote_payload_fields,
                    ..
                }) => *wrote_payload_fields = true,
                Some(FrameKind::List | FrameKind::Scalar) => return Err(err()),
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
    module: &Module,
    effect: Effect,
    state: EffectState<'_>,
    addr: CodeAddr,
) -> Result<(), ModuleError> {
    let action = effect
        .kind
        .frame_action()
        .expect("frame action effects are dispatched here");
    let err = || ModuleError::EffectStackImbalance(addr);
    match action {
        FrameAction::Open(kind) => {
            if *state.pending == PendingState::Full {
                return Err(err());
            }
            let frame = match kind {
                ValueFrameKind::List => FrameKind::List,
                ValueFrameKind::Record => FrameKind::Record,
                ValueFrameKind::Variant => FrameKind::Variant {
                    member: effect.payload as u16,
                    wrote_payload_fields: false,
                },
                ValueFrameKind::Scalar => FrameKind::Scalar,
            };
            state.stack.push(frame);
        }
        FrameAction::Close(ValueFrameKind::Variant) => match state.stack.pop() {
            Some(FrameKind::Variant {
                member,
                wrote_payload_fields,
            }) => {
                let has_no_payload = variant_member_has_no_payload(module, member, addr)?;
                let payload_pending = match *state.pending {
                    PendingState::Full => true,
                    PendingState::Empty => false,
                    PendingState::Unknown => !wrote_payload_fields && !has_no_payload,
                };
                if payload_pending && wrote_payload_fields
                    || (payload_pending || wrote_payload_fields) == has_no_payload
                {
                    return Err(err());
                }
                *state.pending = PendingState::Full;
            }
            _ => return Err(err()),
        },
        FrameAction::Close(kind) => {
            let expected = match kind {
                ValueFrameKind::List => FrameKind::List,
                ValueFrameKind::Record => FrameKind::Record,
                ValueFrameKind::Scalar => FrameKind::Scalar,
                ValueFrameKind::Variant => unreachable!("variant close handled above"),
            };
            if state.stack.pop() != Some(expected) || *state.pending == PendingState::Full {
                return Err(err());
            }
            *state.pending = PendingState::Full;
        }
    }
    Ok(())
}

struct EffectState<'a> {
    stack: &'a mut Vec<FrameKind>,
    suppress: &'a mut i32,
    span_stack: &'a mut Vec<u16>,
    pending: &'a mut PendingState,
    entry_tos: &'a mut u8,
    record_sets_caller_top: &'a mut bool,
}

/// A `SpanEnd` must close the innermost open span, with the id the matching
/// bracket opened — a lone or mis-paired close is malformed bytecode.
fn close_span(span_stack: &mut Vec<u16>, id: u16, addr: CodeAddr) -> Result<(), ModuleError> {
    match span_stack.pop() {
        Some(open) if open == id => Ok(()),
        _ => Err(ModuleError::SpanImbalance(addr)),
    }
}

fn variant_member_has_no_payload(
    module: &Module,
    member: u16,
    addr: CodeAddr,
) -> Result<bool, ModuleError> {
    let types = module.types();
    let type_id = types.member_type_id(member as usize);
    let Some(type_def) = types.get(type_id) else {
        return Err(ModuleError::EffectStackImbalance(addr));
    };
    Ok(matches!(
        type_def.decode(),
        TypeDefKind::Primitive(TypeKind::NoValue)
    ))
}
