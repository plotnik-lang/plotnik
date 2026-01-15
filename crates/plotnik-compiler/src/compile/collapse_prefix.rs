//! Prefix-collapse optimization: merge structurally identical successor instructions.
//!
//! When an instruction has multiple successors that differ only in their successors,
//! we can merge them into one instruction with combined successors.
//!
//! Before:
//!   entry.successors = [A, B]
//!   A: nav=Down, pre=[e1], post=[e2], node_type=Named("x"), successors=[α, β]
//!   B: nav=Down, pre=[e1], post=[e2], node_type=Named("x"), successors=[γ]
//!
//! After:
//!   entry.successors = [A]
//!   A: nav=Down, pre=[e1], post=[e2], node_type=Named("x"), successors=[α, β, γ]
//!   B: unreachable → removed
//!
//! This arises after epsilon elimination when expanded targets are structurally identical.

use std::collections::{HashMap, HashSet};

use crate::bytecode::{InstructionIR, Label, MatchIR};
use crate::compile::CompileResult;

/// Collapse structurally identical successor instructions.
///
/// Uses collect-then-apply to avoid cascading merges from mutation during iteration.
/// Skips processing instructions that are merge targets to avoid conflicting updates.
pub fn collapse_prefix(result: &mut CompileResult) {
    let label_to_idx: HashMap<Label, usize> = result
        .instructions
        .iter()
        .enumerate()
        .map(|(i, instr)| (instr.label(), i))
        .collect();

    // Phase 1a: Identify merge targets (instructions that will receive merged successors)
    let mut merge_targets: HashSet<Label> = HashSet::new();
    for instr in &result.instructions {
        let InstructionIR::Match(m) = instr else {
            continue;
        };
        if m.successors.len() < 2 {
            continue;
        }
        let groups = group_by_structure(&m.successors, &label_to_idx, &result.instructions);
        for group in &groups {
            if group.len() > 1 {
                merge_targets.insert(group[0]);
            }
        }
    }

    // Phase 1b: Collect updates, skipping merge targets
    let mut updates: HashMap<Label, Vec<Label>> = HashMap::new();
    let mut removed: HashSet<Label> = HashSet::new();

    for instr in &result.instructions {
        let InstructionIR::Match(m) = instr else {
            continue;
        };

        // Skip merge targets to avoid conflicting updates
        if merge_targets.contains(&m.label) {
            continue;
        }

        if m.successors.len() < 2 {
            continue;
        }

        let groups = group_by_structure(&m.successors, &label_to_idx, &result.instructions);

        if groups.iter().all(|g| g.len() == 1) {
            continue;
        }

        let mut new_successors = Vec::new();
        for group in groups {
            if group.len() == 1 {
                new_successors.push(group[0]);
            } else {
                let first = group[0];
                let merged_succs: Vec<Label> = group
                    .iter()
                    .flat_map(|&label| {
                        let idx = label_to_idx[&label];
                        result.instructions[idx].successors()
                    })
                    .collect();

                updates.insert(first, merged_succs);
                new_successors.push(first);

                removed.extend(group[1..].iter().copied());
            }
        }

        updates.insert(m.label, new_successors);
    }

    // Phase 2: Apply all updates
    for instr in &mut result.instructions {
        if let InstructionIR::Match(m) = instr
            && let Some(new_succs) = updates.remove(&m.label)
        {
            m.successors = new_succs;
        }
    }

    // Phase 3: Remove absorbed instructions
    result
        .instructions
        .retain(|instr| !removed.contains(&instr.label()));
}

/// Group labels by structural equality of their instructions (excluding successors).
/// Preserves original order within groups.
fn group_by_structure(
    successors: &[Label],
    label_to_idx: &HashMap<Label, usize>,
    instructions: &[InstructionIR],
) -> Vec<Vec<Label>> {
    let mut groups: Vec<Vec<Label>> = Vec::new();

    for &label in successors {
        let Some(&idx) = label_to_idx.get(&label) else {
            groups.push(vec![label]);
            continue;
        };

        let instr = &instructions[idx];

        let found = groups.iter_mut().find(|group| {
            let Some(&first_idx) = label_to_idx.get(&group[0]) else {
                return false;
            };
            structure_eq(&instructions[first_idx], instr)
        });

        if let Some(group) = found {
            group.push(label);
        } else {
            groups.push(vec![label]);
        }
    }

    groups
}

/// Check if two instructions are structurally equal (excluding label and successors).
fn structure_eq(a: &InstructionIR, b: &InstructionIR) -> bool {
    match (a, b) {
        (InstructionIR::Match(a), InstructionIR::Match(b)) => structure_eq_match(a, b),
        (InstructionIR::Call(a), InstructionIR::Call(b)) => {
            a.nav == b.nav && a.node_field == b.node_field && a.target == b.target
        }
        (InstructionIR::Return(_), InstructionIR::Return(_)) => true,
        (InstructionIR::Trampoline(_), InstructionIR::Trampoline(_)) => true,
        _ => false,
    }
}

/// Check if two MatchIR are structurally equal (excluding label and successors).
fn structure_eq_match(a: &MatchIR, b: &MatchIR) -> bool {
    a.nav == b.nav
        && a.node_type == b.node_type
        && a.node_field == b.node_field
        && a.pre_effects == b.pre_effects
        && a.neg_fields == b.neg_fields
        && a.post_effects == b.post_effects
        && a.predicate == b.predicate
}
