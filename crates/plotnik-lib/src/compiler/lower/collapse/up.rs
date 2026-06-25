//! Up-collapse optimization: merge consecutive Up instructions of the same mode.
//!
//! Transforms: Up(1) → Up(1) → Up(2) into Up(4)
//!
//! Merging the constraint-carrying modes (`UpSkipTrivia`/`UpSkipExtras`/`UpExact`)
//! is sound because `Up*` composes: the VM re-validates the exit constraint at
//! every level it ascends (see `the VM`'s `go_up`), so `Up*(a)` then `Up*(b)`
//! is exactly `Up*(a+b)`. A merge that would overflow the level field is refused,
//! leaving a contiguous chain whose per-level checks partition the levels with no
//! gap — never a capped instruction that silently drops upward movement.
//!
//! Constraints:
//! - Same mode only (Up, UpSkipTrivia, UpSkipExtras, UpExact can't mix)
//! - Effectless only (no pre_effects, post_effects, neg_fields)
//! - Capped at `Nav::MAX_UP_LEVEL` per instruction (the 5-bit level field)
//! - Single successor (can't merge branching instructions)

use std::collections::{HashMap, HashSet};

use crate::bytecode::Nav;

use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR, NfaGraph, NodeKindConstraint};

pub fn collapse_up(result: &mut NfaGraph) {
    let label_to_idx: HashMap<Label, usize> = result
        .instructions
        .iter()
        .enumerate()
        .map(|(i, instr)| (instr.label(), i))
        .collect();

    // Only collapse into a successor with exactly one predecessor; others still point to it.
    let mut predecessor_count: HashMap<Label, usize> = HashMap::new();
    for instr in &result.instructions {
        for &succ in instr.successors() {
            *predecessor_count.entry(succ).or_default() += 1;
        }
    }

    let mut removed: HashSet<Label> = HashSet::new();

    for i in 0..result.instructions.len() {
        // An already-absorbed instruction must not seed a new merge: its original
        // successor edge is stale, and reusing it lets a removed node re-absorb the
        // live boundary node a capped chain stopped at, dangling the head's edge.
        if removed.contains(&result.instructions[i].label()) {
            continue;
        }

        let InstructionIR::Match(m) = &result.instructions[i] else {
            continue;
        };

        let Some(up_level) = m.nav.up_level() else {
            continue;
        };

        if m.successors.len() != 1 {
            continue;
        }

        let mut current_level = up_level;
        let mut current_nav = m.nav;
        let mut final_successors = m.successors.clone();

        while current_level < Nav::MAX_UP_LEVEL {
            let &[succ_label] = final_successors.as_slice() else {
                break;
            };

            if removed.contains(&succ_label) {
                break;
            }

            let Some(&succ_idx) = label_to_idx.get(&succ_label) else {
                break;
            };

            let InstructionIR::Match(succ) = &result.instructions[succ_idx] else {
                break;
            };

            let Some(succ_level) = succ.nav.up_level() else {
                break;
            };

            if !current_nav.same_up_mode(succ.nav) || !is_effectless(succ) {
                break;
            }

            if predecessor_count.get(&succ_label).copied().unwrap_or(0) != 1 {
                break;
            }

            // Merge: add levels, but only when the sum is still encodable. If it would
            // overflow the level field, refuse the merge — capping to the max would
            // silently drop upward movement while still absorbing the successor.
            let new_level = current_level.saturating_add(succ_level);
            if new_level > Nav::MAX_UP_LEVEL {
                break;
            }
            current_nav = current_nav.with_up_level(new_level);
            current_level = new_level;
            final_successors = succ.successors.clone();
            removed.insert(succ_label);
        }

        if current_level != up_level {
            let InstructionIR::Match(m) = &mut result.instructions[i] else {
                unreachable!()
            };
            m.nav = current_nav;
            m.successors = final_successors;
        }
    }

    result
        .instructions
        .retain(|instr| !removed.contains(&instr.label()));
}

/// Check if a MatchIR has no effects or constraints (pure navigation).
fn is_effectless(m: &MatchIR) -> bool {
    m.node_kind == NodeKindConstraint::Any
        && m.node_field.is_none()
        && m.pre_effects.is_empty()
        && m.neg_fields.is_empty()
        && m.post_effects.is_empty()
        && m.predicate.is_none()
}
