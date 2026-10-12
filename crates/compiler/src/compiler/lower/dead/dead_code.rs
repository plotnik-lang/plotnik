//! Dead code elimination pass.
//!
//! Removes unreachable instructions and definition specializations after epsilon
//! elimination. Reachability starts at exported definition entries; a call
//! reaches both its return continuations and its callee body.

use std::collections::HashSet;

use crate::compiler::lower::ir::{InstructionIR, Label, NfaGraph};

pub fn remove_unreachable(nfa: &mut NfaGraph) {
    let reachable = compute_reachable(nfa);
    nfa.instructions
        .retain(|instr| reachable.contains(&instr.label()));
    nfa.def_entries.retain(|_, entry| reachable.contains(entry));
}

fn compute_reachable(nfa: &NfaGraph) -> HashSet<Label> {
    let instructions: std::collections::BTreeMap<Label, &InstructionIR> = nfa
        .instructions
        .iter()
        .map(|instr| (instr.label(), instr))
        .collect();

    let mut reachable = HashSet::new();
    let mut queue: Vec<Label> = nfa
        .entry_points
        .values()
        .map(|entry| entry.target)
        .collect();

    while let Some(label) = queue.pop() {
        if !reachable.insert(label) {
            continue;
        }

        let Some(&instruction) = instructions.get(&label) else {
            continue;
        };
        queue.extend(instruction.successors().iter().copied());
        if let InstructionIR::Call(call) = instruction {
            queue.push(call.target);
        }
    }

    reachable
}
