//! Interprocedural effect-stack verifier, run at load.
//!
//! The runtime `ValueMaterializer` is a stack machine over the flat effect
//! sequence of the winning path. Five of its operations panic on an ill-shaped
//! builder stack — `Push`/`ArrayClose` want an `Array` on top, `Set` a `Struct` or
//! `Enum`, `StructClose` a `Struct`, `EnumClose` an `Enum`
//! (`crates/plotnik-lib/src/vm/engine/materializer.rs`) — and the VM's `emit_effect`
//! panics if a `SuppressEnd` underflows the suppression counter
//! (`crates/plotnik-lib/src/vm/engine/vm.rs`). On compiler output these are
//! unreachable by construction; on a forged module that swaps one effect they
//! are not. This pass proves them unreachable for *any* module that passes
//! `Module::load`, so they stay sound loud invariants instead of reachable
//! panics.
//!
//! ## Model
//!
//! The materializer's input is the inline concatenation of every committed
//! `Match`'s effects across `Call`/`Return` boundaries, with the VM's suppression
//! filter applied: `SuppressBegin`/`SuppressEnd` adjust a counter and, while it
//! is positive, every data effect is dropped before the log. So the abstract
//! state is `(stack, suppress, pending)`: a builder-frame stack, a suppression
//! depth, and whether the materializer's pending-value register is full. The
//! walk starts from each entrypoint wrapper and follows `Match` successors,
//! descending through `Call` and resuming at its return address — exactly the
//! edge set that orders effects at runtime.
//!
//! ## Why summaries
//!
//! Inlining does not terminate: captured recursion grows the builder stack one
//! frame per level, opaque recursion grows the suppression counter. But every
//! definition body is, by the compiler's scope discipline, *net-neutral* on the
//! builder stack (it closes every frame it opens) and reads at most the caller's
//! top frame before pushing one of its own. So a body's whole interprocedural
//! effect collapses to a single constraint — the set of caller-top kinds it
//! tolerates (`entry_tos`) — plus the verified facts that it is net-neutral and
//! suppression-balanced. Calls apply that constraint instead of inlining, which
//! both terminates and stays sound. The constraints are computed by a monotone
//! fixpoint (a callee that reads its caller's top before pushing propagates the
//! constraint up to its own callers), then a final pass checks every call site
//! and every entrypoint wrapper against the stabilized constraints.
//!
//! Net-neutrality, no popping below entry, and suppression balance are not
//! assumed — they are verified, so a forged body that violates them is rejected
//! rather than silently mismodeled.

use std::collections::HashMap;

use super::{Instruction, Module, ModuleError};
use crate::bytecode::{Effect, EffectKind, TypeDefKind, TypeKind};

/// Builder frames the materializer pushes. The root/result frame can be a
/// scalar, but compiled effects only push these three frame kinds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

// `entry_tos` is a set of tolerated caller-top kinds, a 3-bit mask.
const KS_ARRAY: u8 = 0b001;
const KS_STRUCT: u8 = 0b010;
const KS_ENUM: u8 = 0b100;
/// No constraint (every kind tolerated): the body never reads its caller's top.
const KS_ANY: u8 = KS_ARRAY | KS_STRUCT | KS_ENUM;
/// `Set` targets — a `Struct` or an `Enum` frame.
const KS_SET: u8 = KS_STRUCT | KS_ENUM;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DefSummary {
    entry_tos: u8,
    returns_pending: Option<bool>,
}

impl DefSummary {
    fn unknown() -> Self {
        Self {
            entry_tos: KS_ANY,
            returns_pending: None,
        }
    }
}

/// Summaries keyed by definition-entry step.
type DefSummaries = HashMap<u16, DefSummary>;

/// Verify that no path can drive the materializer or the suppression counter
/// into a panic. Assumes [`Module::validate_transitions`] has already run, so
/// every `decode_step` and every jump target is safe.
pub(crate) fn validate_effect_stack(module: &Module) -> Result<(), ModuleError> {
    let entrypoints = module.entrypoints();

    let mut defs: Vec<u16> = Vec::new();
    for entrypoint in entrypoints.iter() {
        push_unique(&mut defs, u16::from(entrypoint.target()));
    }

    let mut summaries: DefSummaries = defs.iter().map(|&d| (d, DefSummary::unknown())).collect();

    // Monotone fixpoint: `entry_tos` only ever shrinks (intersection), and the
    // definition set only grows, both within finite bounds, so this terminates.
    loop {
        let mut changed = false;
        let mut i = 0;
        while i < defs.len() {
            let entry = defs[i];
            i += 1;

            let analysis = analyze(module, &summaries, entry, false)?;
            for target in analysis.discovered {
                if push_unique(&mut defs, target) {
                    summaries.insert(target, DefSummary::unknown());
                    changed = true;
                }
            }

            let next = DefSummary {
                entry_tos: analysis.entry_tos,
                returns_pending: analysis.returns_pending,
            };
            let slot = summaries.entry(entry).or_insert(DefSummary::unknown());
            if *slot != next {
                *slot = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // Final pass: with stabilized constraints, check every call site (membership
    // of the caller's top in the callee's `entry_tos`) inside each body...
    for &entry in &defs {
        analyze(module, &summaries, entry, true)?;
    }

    // ...and every entrypoint wrapper. A wrapper has no caller, so a residual
    // caller-top constraint means some effect would read below the frames the
    // wrapper itself opened and hit the materializer's result root frame.
    for entrypoint in entrypoints.iter() {
        let target = u16::from(entrypoint.target());
        let wrapper = analyze(module, &summaries, target, true)?;
        if wrapper.entry_tos != KS_ANY {
            return Err(ModuleError::EffectStackImbalance(target));
        }
    }

    Ok(())
}

struct Analysis {
    /// Caller-top kinds this body tolerates (intersection of every read).
    entry_tos: u8,
    /// Whether every exit from this body leaves a pending value.
    returns_pending: Option<bool>,
    /// Call targets reached — definitions to summarize.
    discovered: Vec<u16>,
}

/// Per-step abstract state at instruction entry: the builder frames pushed so
/// far (relative to this body's entry), suppression depth, and pending register.
type FrameState = (Vec<FrameKind>, i32, PendingState);

/// Walk one body, computing its `entry_tos` and verifying its structural
/// invariants. When `final_check` is set, also verify each call site against
/// `summaries`.
fn analyze(
    module: &Module,
    summaries: &DefSummaries,
    entry: u16,
    final_check: bool,
) -> Result<Analysis, ModuleError> {
    let mut entry_tos = KS_ANY;
    let mut returns_pending = None;
    let mut discovered = Vec::new();
    // Each step is processed once at a concrete abstract state. Recursive calls
    // can temporarily return `Unknown` while summaries converge; a later known
    // state refines that placeholder. Two different known states are a real
    // confluence/back-edge disagreement and are rejected.
    let mut memo: HashMap<u16, FrameState> = HashMap::new();
    let mut work: Vec<(u16, Vec<FrameKind>, i32, PendingState)> =
        vec![(entry, Vec::new(), 0, PendingState::Empty)];

    while let Some((step, stack, suppress, pending)) = work.pop() {
        if let Some((seen_stack, seen_suppress, seen_pending)) = memo.get(&step) {
            if seen_stack != &stack || *seen_suppress != suppress {
                return Err(ModuleError::EffectStackImbalance(step));
            }
            if *seen_pending == pending || pending == PendingState::Unknown {
                continue;
            }
            if *seen_pending != PendingState::Unknown {
                return Err(ModuleError::EffectStackImbalance(step));
            }
        }
        memo.insert(step, (stack.clone(), suppress, pending));

        let mut stack = stack;
        let mut suppress = suppress;
        let mut pending = pending;

        match module.decode_step(step) {
            Instruction::Return(_) => {
                record_exit(&stack, suppress, pending, &mut returns_pending, step)?;
            }
            Instruction::Match(m) => {
                for eff in m.effects() {
                    apply_effect(
                        module,
                        eff,
                        &mut stack,
                        &mut suppress,
                        &mut pending,
                        &mut entry_tos,
                        step,
                    )?;
                }
                if m.succ_count() == 0 {
                    // A successor-less match accepts (unwinds to the top); the
                    // surviving stack must be balanced.
                    record_exit(&stack, suppress, pending, &mut returns_pending, step)?;
                } else {
                    for succ in m.successors() {
                        work.push((u16::from(succ), stack.clone(), suppress, pending));
                    }
                }
            }
            Instruction::Call(c) => {
                let target = u16::from(c.target);
                discovered.push(target);
                apply_call(
                    summaries,
                    target,
                    CallState {
                        stack: &stack,
                        suppress,
                        pending: &mut pending,
                        entry_tos: &mut entry_tos,
                    },
                    final_check,
                    step,
                )?;
                work.push((u16::from(c.next), stack, suppress, pending));
            }
        }
    }

    Ok(Analysis {
        entry_tos,
        returns_pending,
        discovered,
    })
}

/// Apply one effect to the abstract state. Records a caller-top constraint into
/// `entry_tos` when a read happens with no own frame on top, and rejects a
/// frame-kind mismatch, a pop below entry, or a suppression underflow.
fn apply_effect(
    module: &Module,
    effect: Effect,
    stack: &mut Vec<FrameKind>,
    suppress: &mut i32,
    pending: &mut PendingState,
    entry_tos: &mut u8,
    step: u16,
) -> Result<(), ModuleError> {
    use EffectKind::*;

    // Suppression brackets act regardless of depth; everything else is dropped
    // by the VM while suppressed and so must not touch the builder stack here.
    if *suppress > 0 {
        match effect.kind {
            SuppressBegin => *suppress += 1,
            SuppressEnd => *suppress -= 1,
            _ => {}
        }
        return Ok(());
    }

    let err = || ModuleError::EffectStackImbalance(step);
    match effect.kind {
        Node | Null => {
            if *pending == PendingState::Full {
                return Err(err());
            }
            *pending = PendingState::Full;
        }
        SuppressBegin => *suppress += 1,
        // At depth 0 a `SuppressEnd` would drive the counter negative — the
        // exact underflow the VM panics on.
        SuppressEnd => return Err(err()),
        ArrayOpen => {
            if *pending == PendingState::Full {
                return Err(err());
            }
            stack.push(FrameKind::Array);
        }
        StructOpen => {
            if *pending == PendingState::Full {
                return Err(err());
            }
            stack.push(FrameKind::Struct);
        }
        EnumOpen => {
            if *pending == PendingState::Full {
                return Err(err());
            }
            stack.push(FrameKind::Enum {
                member: effect.payload as u16,
                got_data: false,
            });
        }
        Push => {
            if *pending == PendingState::Empty {
                return Err(err());
            }
            match stack.last() {
                Some(FrameKind::Array) => {}
                Some(_) => return Err(err()),
                None => *entry_tos &= KS_ARRAY,
            }
            *pending = PendingState::Empty;
        }
        Set => {
            if *pending == PendingState::Empty {
                return Err(err());
            }
            match stack.last_mut() {
                Some(FrameKind::Struct) => {}
                Some(FrameKind::Enum { got_data, .. }) => *got_data = true,
                Some(FrameKind::Array) => return Err(err()),
                None => *entry_tos &= KS_SET,
            }
            *pending = PendingState::Empty;
        }
        ArrayClose => match stack.pop() {
            Some(FrameKind::Array) if *pending != PendingState::Full => {
                *pending = PendingState::Full
            }
            _ => return Err(err()),
        },
        StructClose => match stack.pop() {
            Some(FrameKind::Struct) if *pending != PendingState::Full => {
                *pending = PendingState::Full
            }
            _ => return Err(err()),
        },
        EnumClose => match stack.pop() {
            Some(FrameKind::Enum { member, got_data }) => {
                let is_void = enum_member_is_void(module, member, step)?;
                let data_pending = match *pending {
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
                *pending = PendingState::Full;
            }
            _ => return Err(err()),
        },
    }
    Ok(())
}

/// Apply a callee summary at a call site: net-neutral on the builder stack, but
/// the callee may read the frame on top before pushing its own. With an own
/// frame on top, check it against the callee's `entry_tos`; with none, the read
/// reaches this body's caller, so propagate the constraint up.
fn apply_call(
    summaries: &DefSummaries,
    target: u16,
    state: CallState<'_>,
    final_check: bool,
    step: u16,
) -> Result<(), ModuleError> {
    if state.suppress > 0 {
        // A suppressed callee is frozen: all its data effects are dropped, so it
        // is a no-op on the builder stack.
        return Ok(());
    }
    if *state.pending == PendingState::Full {
        return Err(ModuleError::EffectStackImbalance(step));
    }

    let summary = summaries
        .get(&target)
        .copied()
        .unwrap_or_else(DefSummary::unknown);
    let constraint = summary.entry_tos;
    match state.stack.last() {
        Some(&k) => {
            if final_check && k.bit() & constraint == 0 {
                return Err(ModuleError::EffectStackImbalance(step));
            }
        }
        None => *state.entry_tos &= constraint,
    }
    *state.pending = summary
        .returns_pending
        .map(PendingState::from_bool)
        .unwrap_or(PendingState::Unknown);
    Ok(())
}

struct CallState<'a> {
    stack: &'a [FrameKind],
    suppress: i32,
    pending: &'a mut PendingState,
    entry_tos: &'a mut u8,
}

/// A body must close every frame it opens and balance every suppression bracket
/// before it returns; otherwise it pops a caller frame or leaks suppression.
fn record_exit(
    stack: &[FrameKind],
    suppress: i32,
    pending: PendingState,
    returns_pending: &mut Option<bool>,
    step: u16,
) -> Result<(), ModuleError> {
    if !stack.is_empty() || suppress != 0 {
        return Err(ModuleError::EffectStackImbalance(step));
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

/// Append `value` if absent; returns whether it was newly inserted.
fn push_unique(defs: &mut Vec<u16>, value: u16) -> bool {
    if defs.contains(&value) {
        false
    } else {
        defs.push(value);
        true
    }
}
