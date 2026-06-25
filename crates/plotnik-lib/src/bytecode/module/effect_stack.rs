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
//! `Match`'s `pre` then `post` effects across `Call`/`Return` boundaries, with
//! the VM's suppression filter applied: `SuppressBegin`/`SuppressEnd` adjust a
//! counter and, while it is positive, every data effect is dropped before the
//! log. So the abstract state is `(stack, suppress)`: a builder-frame stack and
//! a suppression depth. The walk starts from the preamble (step 0) and follows
//! `Match` successors, descending through `Call`/`Trampoline` and resuming at
//! their return address — exactly the edge set that orders effects at runtime.
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
//! and the preamble against the stabilized constraints.
//!
//! Net-neutrality, no popping below entry, and suppression balance are not
//! assumed — they are verified, so a forged body that violates them is rejected
//! rather than silently mismodeled.

use std::collections::HashMap;

use super::{Instruction, Module, ModuleError};
use crate::bytecode::StepAddr;
use crate::bytecode::effects::EffectKind;

/// Builder frames the materializer pushes. The root/result frame can be a
/// scalar, but the walk starts at the always-`StructOpen` preamble, so only these three
/// ever reach the abstract stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FrameKind {
    Array,
    Struct,
    Enum,
}

impl FrameKind {
    fn bit(self) -> u8 {
        match self {
            FrameKind::Array => KS_ARRAY,
            FrameKind::Struct => KS_STRUCT,
            FrameKind::Enum => KS_ENUM,
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

/// Summaries keyed by definition-entry step. The value is the `entry_tos` mask.
type DefSummaries = HashMap<u16, u8>;

/// Verify that no path can drive the materializer or the suppression counter
/// into a panic. Assumes [`Module::validate_transitions`] has already run, so
/// every `decode_step` and every jump target is safe.
pub(crate) fn validate_effect_stack(module: &Module) -> Result<(), ModuleError> {
    let entrypoints = module.entrypoints();

    let mut defs: Vec<u16> = Vec::new();
    for entrypoint in entrypoints.iter() {
        push_unique(&mut defs, u16::from(entrypoint.target()));
    }

    let mut summaries: DefSummaries = defs.iter().map(|&d| (d, KS_ANY)).collect();

    // Monotone fixpoint: `entry_tos` only ever shrinks (intersection), and the
    // definition set only grows, both within finite bounds, so this terminates.
    loop {
        let mut changed = false;
        let mut i = 0;
        while i < defs.len() {
            let entry = defs[i];
            i += 1;

            let analysis = analyze(module, &summaries, entry, None, false)?;
            for target in analysis.discovered {
                if push_unique(&mut defs, target) {
                    summaries.insert(target, KS_ANY);
                    changed = true;
                }
            }

            let slot = summaries.entry(entry).or_insert(KS_ANY);
            if *slot != analysis.entry_tos {
                *slot = analysis.entry_tos;
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
        analyze(module, &summaries, entry, None, true)?;
    }

    // ...and the preamble per entrypoint. The shared preamble (step 0) opens the
    // root `StructOpen`, trampolines into the entrypoint body, then closes it; binding
    // the trampoline to this entrypoint's target checks that the body tolerates
    // the `Struct` the preamble hands it.
    //
    // The preamble has no caller. A residual constraint on *its* entry means some
    // effect read below the frames the preamble itself opened — with the root
    // `StructOpen` intact, every entry read lands on that `Struct` and nothing bubbles,
    // so a non-`KS_ANY` result is a forged preamble that fails to provide the
    // frame. At runtime such a read would hit the materializer's result-type root
    // frame, whose kind is attacker-controlled, and panic. Reject it instead of
    // dropping the constraint into a caller that does not exist.
    for entrypoint in entrypoints.iter() {
        let target = u16::from(entrypoint.target());
        let preamble = analyze(
            module,
            &summaries,
            StepAddr::PREAMBLE.get(),
            Some(target),
            true,
        )?;
        if preamble.entry_tos != KS_ANY {
            return Err(ModuleError::EffectStackImbalance(StepAddr::PREAMBLE.get()));
        }
    }

    Ok(())
}

struct Analysis {
    /// Caller-top kinds this body tolerates (intersection of every read).
    entry_tos: u8,
    /// Call/trampoline targets reached — definitions to summarize.
    discovered: Vec<u16>,
}

/// Per-step abstract state at instruction entry: the builder frames pushed so
/// far (relative to this body's entry) and the suppression depth.
type FrameState = (Vec<FrameKind>, i32);

/// Walk one body (a definition entry, or the preamble when `trampoline` is set),
/// computing its `entry_tos` and verifying its structural invariants. When
/// `final_check` is set, also verify each call site against `summaries`.
fn analyze(
    module: &Module,
    summaries: &DefSummaries,
    entry: u16,
    trampoline: Option<u16>,
    final_check: bool,
) -> Result<Analysis, ModuleError> {
    let mut entry_tos = KS_ANY;
    let mut discovered = Vec::new();
    // Each step is processed once, at a single abstract state; a second arrival
    // with a different state is a confluence/back-edge disagreement (forged or
    // not loop-invariant) and is rejected. This bounds the walk to one state per
    // step, so it terminates.
    let mut memo: HashMap<u16, FrameState> = HashMap::new();
    let mut work: Vec<(u16, Vec<FrameKind>, i32)> = vec![(entry, Vec::new(), 0)];

    while let Some((step, stack, suppress)) = work.pop() {
        if let Some((seen_stack, seen_suppress)) = memo.get(&step) {
            if seen_stack == &stack && *seen_suppress == suppress {
                continue;
            }
            return Err(ModuleError::EffectStackImbalance(step));
        }
        memo.insert(step, (stack.clone(), suppress));

        let mut stack = stack;
        let mut suppress = suppress;

        match module.decode_step(step) {
            Instruction::Return(_) => {
                require_neutral(&stack, suppress, step)?;
            }
            Instruction::Match(m) => {
                for eff in m.pre_effects().chain(m.post_effects()) {
                    apply_effect(eff.kind, &mut stack, &mut suppress, &mut entry_tos, step)?;
                }
                if m.succ_count() == 0 {
                    // A successor-less match accepts (unwinds to the top); the
                    // surviving stack must be balanced.
                    require_neutral(&stack, suppress, step)?;
                } else {
                    for succ in m.successors() {
                        work.push((u16::from(succ), stack.clone(), suppress));
                    }
                }
            }
            Instruction::Call(c) => {
                let target = u16::from(c.target);
                discovered.push(target);
                apply_call(
                    summaries,
                    target,
                    &stack,
                    suppress,
                    &mut entry_tos,
                    final_check,
                    step,
                )?;
                work.push((u16::from(c.next), stack, suppress));
            }
            Instruction::Trampoline(t) => {
                // The trampoline's callee is the entrypoint bound by the caller;
                // it only appears in the preamble.
                let target = trampoline.ok_or(ModuleError::EffectStackImbalance(step))?;
                discovered.push(target);
                apply_call(
                    summaries,
                    target,
                    &stack,
                    suppress,
                    &mut entry_tos,
                    final_check,
                    step,
                )?;
                work.push((u16::from(t.next()), stack, suppress));
            }
        }
    }

    Ok(Analysis {
        entry_tos,
        discovered,
    })
}

/// Apply one effect to the abstract state. Records a caller-top constraint into
/// `entry_tos` when a read happens with no own frame on top, and rejects a
/// frame-kind mismatch, a pop below entry, or a suppression underflow.
fn apply_effect(
    op: EffectKind,
    stack: &mut Vec<FrameKind>,
    suppress: &mut i32,
    entry_tos: &mut u8,
    step: u16,
) -> Result<(), ModuleError> {
    use EffectKind::*;

    // Suppression brackets act regardless of depth; everything else is dropped
    // by the VM while suppressed and so must not touch the builder stack here.
    if *suppress > 0 {
        match op {
            SuppressBegin => *suppress += 1,
            SuppressEnd => *suppress -= 1,
            _ => {}
        }
        return Ok(());
    }

    let err = || ModuleError::EffectStackImbalance(step);
    match op {
        Node | Null => {}
        SuppressBegin => *suppress += 1,
        // At depth 0 a `SuppressEnd` would drive the counter negative — the
        // exact underflow the VM panics on.
        SuppressEnd => return Err(err()),
        ArrayOpen => stack.push(FrameKind::Array),
        StructOpen => stack.push(FrameKind::Struct),
        EnumOpen => stack.push(FrameKind::Enum),
        Push => match stack.last() {
            Some(FrameKind::Array) => {}
            Some(_) => return Err(err()),
            None => *entry_tos &= KS_ARRAY,
        },
        Set => match stack.last() {
            Some(FrameKind::Struct | FrameKind::Enum) => {}
            Some(FrameKind::Array) => return Err(err()),
            None => *entry_tos &= KS_SET,
        },
        ArrayClose => match stack.pop() {
            Some(FrameKind::Array) => {}
            _ => return Err(err()),
        },
        StructClose => match stack.pop() {
            Some(FrameKind::Struct) => {}
            _ => return Err(err()),
        },
        EnumClose => match stack.pop() {
            Some(FrameKind::Enum) => {}
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
    stack: &[FrameKind],
    suppress: i32,
    entry_tos: &mut u8,
    final_check: bool,
    step: u16,
) -> Result<(), ModuleError> {
    if suppress > 0 {
        // A suppressed callee is frozen: all its data effects are dropped, so it
        // is a no-op on the builder stack.
        return Ok(());
    }
    let constraint = summaries.get(&target).copied().unwrap_or(KS_ANY);
    match stack.last() {
        Some(&k) => {
            if final_check && k.bit() & constraint == 0 {
                return Err(ModuleError::EffectStackImbalance(step));
            }
        }
        None => *entry_tos &= constraint,
    }
    Ok(())
}

/// A body must close every frame it opens and balance every suppression bracket
/// before it returns; otherwise it pops a caller frame or leaks suppression.
fn require_neutral(stack: &[FrameKind], suppress: i32, step: u16) -> Result<(), ModuleError> {
    if stack.is_empty() && suppress == 0 {
        Ok(())
    } else {
        Err(ModuleError::EffectStackImbalance(step))
    }
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
