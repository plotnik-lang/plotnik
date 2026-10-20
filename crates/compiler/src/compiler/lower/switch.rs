use std::collections::{HashMap, HashSet};

use crate::bytecode::Nav;
use crate::core::NodeKindId;

use super::ir::{InstructionIR, Label, NfaGraph, SwitchIR};

pub(super) fn form_switches(graph: &mut NfaGraph) {
    let instructions: HashMap<_, _> = graph
        .instructions
        .iter()
        .map(|instruction| (instruction.label(), instruction))
        .collect();
    let mut replacements = Vec::new();

    for (index, instruction) in graph.instructions.iter().enumerate() {
        let InstructionIR::Match(fork) = instruction else {
            continue;
        };

        if !fork.is_epsilon() || !fork.effects.is_empty() || fork.successors.len() < 2 {
            continue;
        }

        let mut kinds = HashSet::new();
        let cases: Option<Vec<_>> = fork
            .successors
            .iter()
            .map(|&target| {
                let kind = entry_kind(target, &instructions)?;
                kinds.insert(kind).then_some((kind, target))
            })
            .collect();

        if let Some(cases) = cases {
            replacements.push((index, SwitchIR::new(fork.label, cases)));
        }
    }

    for (index, switch) in replacements {
        graph.instructions[index] = InstructionIR::Switch(switch);
    }
}

fn entry_kind(
    mut label: Label,
    instructions: &HashMap<Label, &InstructionIR>,
) -> Option<NodeKindId> {
    let mut visited = HashSet::new();
    loop {
        if !visited.insert(label) {
            return None;
        }

        let InstructionIR::Match(instruction) = instructions[&label] else {
            return None;
        };

        if instruction.is_epsilon() {
            let [next] = instruction.successors.as_slice() else {
                return None;
            };

            label = *next;
            continue;
        }

        if instruction.nav != Nav::StayExact {
            return None;
        }

        return instruction.node_kind.kind_id();
    }
}
