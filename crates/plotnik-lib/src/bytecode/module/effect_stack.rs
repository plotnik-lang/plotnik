//! Interprocedural effect-stack verifier, run at load.
//!
//! The runtime `ValueMaterializer` is a stack machine over the flat effect
//! sequence of the winning path. Five of its operations panic on an ill-shaped
//! builder stack — `Push`/`ArrayClose` want an `Array` on top, `Set` a `Struct` or
//! `Enum`, `StructClose` a `Struct`, `EnumClose` an `Enum`
//! (`crates/plotnik-lib/src/vm/engine/materializer.rs`) — plus `EnumClose`
//! panics when a payload arrives both as the pending value and as direct
//! fields, the end-of-log assert panics on unclosed frames, and the VM's
//! `emit_effect` panics if a `SuppressEnd` underflows the suppression counter
//! (`crates/plotnik-lib/src/vm/engine/vm.rs`). On compiler output these are
//! unreachable by construction; on a forged module that swaps one effect they
//! are not. This pass proves them unreachable for *any* module that passes
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
//! walk starts from each entrypoint wrapper and follows `Match` successors,
//! descending through `Call` and resuming at its return address — exactly the
//! edge set that orders effects at runtime.
//!
//! ## Why state *sets* (collecting semantics)
//!
//! The panic-freedom property is per *path*: every graph path must replay to a
//! well-shaped effect sequence. Distinct paths may legally reach the same step
//! with different abstract states — the dedup pass hash-conses structurally
//! identical branch tails, so e.g. two enum branches share one `EnumClose`
//! step, reached once under `Enum(A)` and once under `Enum(B)`. Each arrival
//! is individually sound (dedup is a bisimulation quotient: it preserves the
//! op-labeled path set exactly), so the walk keeps a *set* of states per step
//! and verifies every effect against every state that reaches it. Requiring a
//! single state per step — as this pass once did — rejected modules the
//! compiler itself emitted.
//!
//! Termination against forged modules cannot rely on the state sets converging
//! on their own — a net-growing cycle would mint a deeper stack every lap. Two
//! bounds close that off, both loose enough that compiler output never trips
//! them:
//!
//! - **Derived depth bounds.** A state's frame stack can never legitimately be
//!   deeper than the total number of frame-opening effects on the steps the
//!   walk has visited: every push comes from a visited step, and revisiting a
//!   step at a strictly greater depth proves a net-positive cycle, which can
//!   never rebalance — every exit requires an empty local stack, so some path
//!   through such a cycle is provably ill-formed. The suppression counter and
//!   span stack get the same treatment against their own opener counts.
//! - **A state budget.** Frame payloads (enum member, `got_data`, pending) are
//!   finite but can multiply across nesting; a hard cap on states explored per
//!   body bounds load time on adversarial input. Compiler output stays far
//!   below it: the states at a merged step correspond to the pre-dedup twins,
//!   and step addresses are `u16`, so a body contributes at most tens of
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
//! tolerates (`entry_tos`), whether it may `Set` into the caller's top frame
//! (`sets_caller_top`), and the pending state it returns — plus the verified
//! facts that it is net-neutral and suppression-balanced. Calls apply that
//! summary instead of inlining, which both terminates and stays sound. The
//! summaries are computed by a monotone fixpoint (a callee that reads its
//! caller's top before pushing propagates the constraint up to its own
//! callers), then a final pass checks every call site and every entrypoint
//! wrapper against the stabilized summaries.
//!
//! `sets_caller_top` exists because a below-entry `Set` mutates state the
//! caller's walk otherwise cannot see: setting a field on the caller's *enum*
//! frame flips the data the frame will carry at its `EnumClose`. A call site
//! whose local top is an enum therefore forks the state — one branch assumes
//! the write happened, one that it did not — so a stale `got_data` can never
//! smuggle a "payload arrived both as pending value and as direct fields"
//! panic past the check.
//!
//! A successor-less `Match` accepts the *whole run* from any call depth,
//! freezing the log with every caller frame still open. Inside an entrypoint
//! wrapper the local stack is the global stack, so the existing exit check is
//! exact; inside a body reachable through `Call` the caller's frames are
//! invisible here, so such accepts are rejected outright — the compiler ends
//! every definition body with `Return` and only accepts at wrapper level.
//!
//! Net-neutrality, no popping below entry, and suppression balance are not
//! assumed — they are verified, so a forged body that violates them is rejected
//! rather than silently mismodeled.
//!
//! ## Scope
//!
//! This pass proves the release-build panic surface unreachable, plus the
//! span-pairing and enum void/data-consistency assertions. Full agreement
//! between materialized values and the declared type tables (checked by
//! `debug_verify_type` in debug builds) is a compiler self-check, not a load
//! guarantee: a forged module can still declare types its effects do not
//! produce.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{Instruction, Module, ModuleError};
use crate::bytecode::{Effect, EffectKind, TypeDefKind, TypeKind};

/// Builder frames the materializer pushes. The root/result frame can be a
/// scalar, but compiled effects only push these three frame kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum FrameKind {
    Array,
    Struct,
    Enum { member: u16, got_data: bool },
}

impl FrameKind {
    fn bit(self) -> u8 {
        match self {
            FrameKind::Array => KS_ARRAY,
            FrameKind::Struct => KS_STRUCT,
            FrameKind::Enum { .. } => KS_ENUM,
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
const KS_ARRAY: u8 = 0b001;
const KS_STRUCT: u8 = 0b010;
const KS_ENUM: u8 = 0b100;
/// No constraint (every kind tolerated): the body never reads its caller's top.
const KS_ANY: u8 = KS_ARRAY | KS_STRUCT | KS_ENUM;
/// `Set` targets — a `Struct` or an `Enum` frame.
const KS_SET: u8 = KS_STRUCT | KS_ENUM;

/// Hard cap on abstract states explored per body walk. Compiler output is
/// bounded by the pre-dedup instruction count (`u16` step space); only a
/// forged module engineering a combinatorial frame-payload blowup can get
/// near this, and rejecting it bounds load time.
const STATE_BUDGET: usize = 1 << 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefSummary {
    entry_tos: u8,
    returns_pending: Option<bool>,
    /// Some path in the body `Set`s the caller's top frame (directly or via a
    /// transitive callee). Call sites must account for the write both having
    /// and not having happened.
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

/// Summaries keyed by definition-entry step.
type DefSummaries = HashMap<u16, DefSummary>;

/// Verify that no path can drive the materializer or the suppression counter
/// into a panic. Assumes [`Module::validate_transitions`] has already run, so
/// every `decode_step` and every jump target is safe.
pub(crate) fn validate_effect_stack(module: &Module) -> Result<(), ModuleError> {
    let entrypoints = module.entrypoints();

    let mut defs = Vec::new();
    let mut known = HashSet::new();
    let mut summaries = DefSummaries::new();
    let mut queue = VecDeque::new();
    let mut queued = HashSet::new();
    for entrypoint in entrypoints.iter() {
        let target = u16::from(entrypoint.target());
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
    let mut callers: HashMap<u16, Vec<u16>> = HashMap::new();
    let mut call_edges = HashSet::new();

    // Monotone fixpoint: `entry_tos` only ever shrinks (intersection),
    // `sets_caller_top` only flips on, `returns_pending` only becomes known,
    // and the definition/called sets only grow. FIFO scheduling coalesces a
    // batch of callee changes before a wide caller is revisited.
    while let Some(entry) = queue.pop_front() {
        queued.remove(&entry);

        let analysis = analyze(module, &summaries, entry, called.contains(&entry), false)?;
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
            sets_caller_top: analysis.sets_caller_top,
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
        analyze(module, &summaries, entry, called.contains(&entry), true)?;
    }

    // ...and every entrypoint wrapper. A wrapper has no caller, so a residual
    // caller-top constraint means some effect would read below the frames the
    // wrapper itself opened and hit the materializer's result root frame.
    for entrypoint in entrypoints.iter() {
        let target = u16::from(entrypoint.target());
        let wrapper = analyze(module, &summaries, target, called.contains(&target), true)?;
        if wrapper.entry_tos != KS_ANY {
            return Err(ModuleError::EffectStackImbalance(target));
        }
    }

    Ok(())
}

fn enqueue(queue: &mut VecDeque<u16>, queued: &mut HashSet<u16>, entry: u16) {
    if queued.insert(entry) {
        queue.push_back(entry);
    }
}

struct Analysis {
    /// Caller-top kinds this body tolerates (intersection of every read).
    entry_tos: u8,
    /// Whether every exit from this body leaves a pending value.
    returns_pending: Option<bool>,
    /// Whether some path `Set`s into the caller's top frame.
    sets_caller_top: bool,
    /// Call targets reached — definitions to summarize.
    discovered: Vec<u16>,
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
}

/// Walk one body, computing its summary facts and verifying its structural
/// invariants against every abstract state that reaches each step. When
/// `final_check` is set, also verify each call site against `summaries`.
fn analyze(
    module: &Module,
    summaries: &DefSummaries,
    entry: u16,
    is_called: bool,
    final_check: bool,
) -> Result<Analysis, ModuleError> {
    let mut entry_tos = KS_ANY;
    let mut returns_pending = None;
    let mut sets_caller_top = false;
    let mut discovered = Vec::new();
    let mut discovered_set = HashSet::new();

    // Collecting semantics: every distinct abstract state a step is reached
    // with is kept and processed once. The opener tallies accumulate, per
    // visited step, how many frame/suppression/span openers exist — a state
    // outgrowing them proves a net-positive cycle (see module docs).
    let mut memo: HashMap<u16, HashSet<AbsState>> = HashMap::new();
    let mut states_spent: usize = 0;
    let mut frame_openers: usize = 0;
    let mut suppress_openers: i32 = 0;
    let mut span_openers: usize = 0;

    let mut work: Vec<(u16, AbsState)> = vec![(entry, AbsState::initial())];

    while let Some((step, state)) = work.pop() {
        let instruction = module.decode_step(step);

        let seen = memo.entry(step).or_insert_with(|| {
            if let Instruction::Match(m) = &instruction {
                for eff in m.effects() {
                    match eff.kind {
                        EffectKind::ArrayOpen | EffectKind::StructOpen | EffectKind::EnumOpen => {
                            frame_openers += 1;
                        }
                        EffectKind::SuppressBegin => suppress_openers += 1,
                        EffectKind::SpanStartAt | EffectKind::SpanStart => span_openers += 1,
                        _ => {}
                    }
                }
            }
            HashSet::new()
        });
        if !seen.insert(state.clone()) {
            continue;
        }
        if state.stack.len() > frame_openers || state.suppress > suppress_openers {
            return Err(ModuleError::EffectStackImbalance(step));
        }
        if state.span_stack.len() > span_openers {
            return Err(ModuleError::SpanImbalance(step));
        }
        states_spent += 1;
        if states_spent > STATE_BUDGET {
            return Err(ModuleError::EffectStackBudget(step));
        }
        let AbsState {
            mut stack,
            mut suppress,
            mut span_stack,
            mut pending,
        } = state;

        match instruction {
            Instruction::Return(_) => {
                record_exit(
                    &stack,
                    suppress,
                    &span_stack,
                    pending,
                    &mut returns_pending,
                    step,
                )?;
            }
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
                            sets_caller_top: &mut sets_caller_top,
                        },
                        step,
                    )?;
                }
                if m.succ_count() == 0 {
                    // A successor-less match accepts the whole run. At wrapper
                    // level the local stack is the global stack, so balance
                    // here is exact; under a `Call` the caller's frames are
                    // still open in the log, so this is never sound.
                    if is_called {
                        return Err(ModuleError::EffectStackImbalance(step));
                    }
                    if !stack.is_empty() || suppress != 0 {
                        return Err(ModuleError::EffectStackImbalance(step));
                    }
                    if !span_stack.is_empty() {
                        return Err(ModuleError::SpanImbalance(step));
                    }
                } else {
                    for succ in m.successors() {
                        work.push((
                            u16::from(succ),
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
            Instruction::Call(c) => {
                let target = u16::from(c.target);
                if discovered_set.insert(target) {
                    discovered.push(target);
                }
                let next = u16::from(c.next);

                if suppress > 0 {
                    // A suppressed callee is frozen: all its data effects are
                    // dropped, so it is a no-op on the builder stack.
                    work.push((
                        next,
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
                    return Err(ModuleError::EffectStackImbalance(step));
                }

                let summary = summaries
                    .get(&target)
                    .copied()
                    .unwrap_or_else(DefSummary::unknown);
                match stack.last() {
                    Some(&k) => {
                        if final_check && k.bit() & summary.entry_tos == 0 {
                            return Err(ModuleError::EffectStackImbalance(step));
                        }
                    }
                    None => {
                        // The callee's reads and writes land on *our* caller's
                        // top frame: inherit the constraint and the write flag.
                        entry_tos &= summary.entry_tos;
                        if summary.sets_caller_top {
                            sets_caller_top = true;
                        }
                    }
                }

                let post_pending = summary
                    .returns_pending
                    .map(PendingState::from_bool)
                    .unwrap_or(PendingState::Unknown);

                // A callee that may `Set` our top enum frame forks the state:
                // the continuation must be sound whether or not the write
                // happened, or a stale `got_data` would mask the materializer's
                // pending-plus-fields panic at the eventual `EnumClose`.
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
                        next,
                        AbsState {
                            stack: written,
                            suppress,
                            span_stack: span_stack.clone(),
                            pending: post_pending,
                        },
                    ));
                }

                work.push((
                    next,
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

    Ok(Analysis {
        entry_tos,
        returns_pending,
        sets_caller_top,
        discovered,
    })
}

/// Apply one effect to the abstract state. Records a caller-top constraint into
/// `entry_tos` when a read happens with no own frame on top, and rejects a
/// frame-kind mismatch, a pop below entry, or a suppression underflow.
fn apply_effect(
    module: &Module,
    effect: Effect,
    state: EffectState<'_>,
    step: u16,
) -> Result<(), ModuleError> {
    use EffectKind::*;

    // Suppression and span brackets act regardless of depth; data effects are
    // dropped by the VM while suppressed and so must not touch the builder stack.
    if *state.suppress > 0 {
        match effect.kind {
            SuppressBegin => *state.suppress += 1,
            SuppressEnd => *state.suppress -= 1,
            SpanStartAt | SpanStart => state.span_stack.push(effect.payload as u16),
            SpanEnd => close_span(state.span_stack, effect.payload as u16, step)?,
            _ => {}
        }
        return Ok(());
    }

    let err = || ModuleError::EffectStackImbalance(step);
    match effect.kind {
        Node | Null => {
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
        SpanEnd => close_span(state.span_stack, effect.payload as u16, step)?,
        ArrayOpen => {
            if *state.pending == PendingState::Full {
                return Err(err());
            }
            state.stack.push(FrameKind::Array);
        }
        StructOpen => {
            if *state.pending == PendingState::Full {
                return Err(err());
            }
            state.stack.push(FrameKind::Struct);
        }
        EnumOpen => {
            if *state.pending == PendingState::Full {
                return Err(err());
            }
            state.stack.push(FrameKind::Enum {
                member: effect.payload as u16,
                got_data: false,
            });
        }
        Push => {
            if *state.pending == PendingState::Empty {
                return Err(err());
            }
            match state.stack.last() {
                Some(FrameKind::Array) => {}
                Some(_) => return Err(err()),
                None => *state.entry_tos &= KS_ARRAY,
            }
            *state.pending = PendingState::Empty;
        }
        Set => {
            if *state.pending == PendingState::Empty {
                return Err(err());
            }
            match state.stack.last_mut() {
                Some(FrameKind::Struct) => {}
                Some(FrameKind::Enum { got_data, .. }) => *got_data = true,
                Some(FrameKind::Array) => return Err(err()),
                None => {
                    *state.entry_tos &= KS_SET;
                    *state.sets_caller_top = true;
                }
            }
            *state.pending = PendingState::Empty;
        }
        ArrayClose => match state.stack.pop() {
            Some(FrameKind::Array) if *state.pending != PendingState::Full => {
                *state.pending = PendingState::Full
            }
            _ => return Err(err()),
        },
        StructClose => match state.stack.pop() {
            Some(FrameKind::Struct) if *state.pending != PendingState::Full => {
                *state.pending = PendingState::Full
            }
            _ => return Err(err()),
        },
        EnumClose => match state.stack.pop() {
            Some(FrameKind::Enum { member, got_data }) => {
                let is_void = enum_member_is_void(module, member, step)?;
                let data_pending = match *state.pending {
                    PendingState::Full => true,
                    PendingState::Empty => false,
                    PendingState::Unknown => !got_data && !is_void,
                };
                if data_pending && got_data {
                    return Err(err());
                }
                let data_present = data_pending || got_data;
                if data_present == is_void {
                    return Err(err());
                }
                *state.pending = PendingState::Full;
            }
            _ => return Err(err()),
        },
    }
    Ok(())
}

struct EffectState<'a> {
    stack: &'a mut Vec<FrameKind>,
    suppress: &'a mut i32,
    span_stack: &'a mut Vec<u16>,
    pending: &'a mut PendingState,
    entry_tos: &'a mut u8,
    sets_caller_top: &'a mut bool,
}

/// A `SpanEnd` must close the innermost open span, with the id the matching
/// bracket opened — a lone or mis-paired close is a forged module.
fn close_span(span_stack: &mut Vec<u16>, id: u16, step: u16) -> Result<(), ModuleError> {
    match span_stack.pop() {
        Some(open) if open == id => Ok(()),
        _ => Err(ModuleError::SpanImbalance(step)),
    }
}

/// A body must close every frame it opens and balance every suppression bracket
/// before it returns; otherwise it pops a caller frame or leaks suppression.
fn record_exit(
    stack: &[FrameKind],
    suppress: i32,
    span_stack: &[u16],
    pending: PendingState,
    returns_pending: &mut Option<bool>,
    step: u16,
) -> Result<(), ModuleError> {
    if !stack.is_empty() || suppress != 0 {
        return Err(ModuleError::EffectStackImbalance(step));
    }
    if !span_stack.is_empty() {
        return Err(ModuleError::SpanImbalance(step));
    }

    if let Some(pending) = pending.known() {
        if let Some(seen) = *returns_pending
            && seen != pending
        {
            return Err(ModuleError::EffectStackImbalance(step));
        }
        *returns_pending = Some(pending);
    }
    Ok(())
}

fn enum_member_is_void(module: &Module, member: u16, step: u16) -> Result<bool, ModuleError> {
    let types = module.types();
    let type_id = types.member_type_id(member as usize);
    let Some(type_def) = types.get(type_id) else {
        return Err(ModuleError::EffectStackImbalance(step));
    };
    Ok(matches!(
        type_def.decode(),
        TypeDefKind::Primitive(TypeKind::Void)
    ))
}
