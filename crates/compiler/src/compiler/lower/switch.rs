use std::collections::{HashMap, HashSet};

use crate::bytecode::Nav;
use crate::core::NodeKindId;

use super::ir::{InstructionIR, Label, MatchIR, NfaGraph, SwitchIR};

pub(crate) struct SwitchEntry<'a> {
    pub(crate) kind: NodeKindId,
    pub(crate) target: Label,
    pub(crate) prefix: Vec<&'a MatchIR>,
    pub(crate) matched: &'a MatchIR,
}

impl SwitchIR {
    pub(crate) fn entries<'a>(
        &self,
        instructions: &HashMap<Label, &'a InstructionIR>,
    ) -> Vec<SwitchEntry<'a>> {
        assert!(
            self.cases().len() >= 2,
            "switch {:?} requires at least two cases",
            self.label
        );
        let mut kinds = HashSet::new();
        self.cases()
            .map(|(kind, target)| {
                assert!(
                    kinds.insert(kind),
                    "switch {:?} repeats kind {kind:?} at {target:?}",
                    self.label
                );
                let (prefix, matched) = entry_path(target, instructions).unwrap_or_else(|| {
                    panic!(
                        "switch {:?} entry {target:?} for kind {kind:?} must reach an exact concrete match",
                        self.label
                    )
                });
                assert_eq!(
                    matched.node_kind.kind_id(),
                    Some(kind),
                    "switch {:?} entry {target:?} must match kind {kind:?}",
                    self.label
                );
                SwitchEntry {
                    kind,
                    target,
                    prefix,
                    matched,
                }
            })
            .collect()
    }
}

fn entry_path<'a>(
    mut label: Label,
    instructions: &HashMap<Label, &'a InstructionIR>,
) -> Option<(Vec<&'a MatchIR>, &'a MatchIR)> {
    let mut visited = HashSet::new();
    let mut prefix = Vec::new();
    loop {
        if !visited.insert(label) {
            return None;
        }

        let InstructionIR::Match(instruction) = *instructions
            .get(&label)
            .expect("every graph target names an instruction")
        else {
            return None;
        };
        if !instruction.is_epsilon() {
            return (instruction.nav == Nav::StayExact
                && instruction.node_kind.kind_id().is_some())
            .then_some((prefix, instruction));
        }

        let [next] = instruction.successors.as_slice() else {
            return None;
        };
        prefix.push(instruction);
        label = *next;
    }
}

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
                let (_, endpoint) = entry_path(target, &instructions)?;
                let kind = endpoint
                    .node_kind
                    .kind_id()
                    .expect("switch endpoint has a concrete kind");
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
