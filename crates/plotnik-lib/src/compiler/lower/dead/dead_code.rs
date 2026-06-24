//! Dead code elimination pass.
//!
//! Removes unreachable instructions after epsilon elimination.
//! Instructions become unreachable when epsilon transitions are
//! bypassed and no other path leads to them.

use std::collections::HashSet;

use crate::compiler::lower::ir::{Label, NfaGraph};

pub fn remove_unreachable(result: &mut NfaGraph) {
    let reachable = compute_reachable(result);
    result
        .instructions
        .retain(|instr| reachable.contains(&instr.label()));
}

fn compute_reachable(result: &NfaGraph) -> HashSet<Label> {
    let successors: std::collections::BTreeMap<Label, Vec<Label>> = result
        .instructions
        .iter()
        .map(|instr| (instr.label(), instr.successors().to_vec()))
        .collect();

    let mut reachable = HashSet::new();
    let mut queue: Vec<Label> = vec![result.preamble_entry];
    queue.extend(result.def_entries.values().copied());

    while let Some(label) = queue.pop() {
        if !reachable.insert(label) {
            continue;
        }
        if let Some(succs) = successors.get(&label) {
            queue.extend(succs.iter().copied());
        }
    }

    reachable
}

#[cfg(test)]
#[path = "dead_code_tests.rs"]
mod dead_code_tests;
