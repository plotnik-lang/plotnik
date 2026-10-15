use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::bytecode::{Nav, NodeKindConstraint};
use crate::core::NodeFieldId;

use crate::compiler::lower::ir::{
    CallEntry, EffectIR, InstructionIR, Label, MatchIR, NfaGraph, PredicateIR,
};

pub fn dedup_states(nfa: &mut NfaGraph) {
    loop {
        dedup_successors(&mut nfa.instructions);

        let mut remap = plan_merges(&nfa.instructions, SuccNorm::SelfLoop);
        if remap.is_empty() {
            remap = plan_merges(&nfa.instructions, SuccNorm::Raw);
        }
        if remap.is_empty() {
            break;
        }

        apply_remap(nfa, &remap);
        nfa.instructions
            .retain(|instr| !remap.contains_key(&instr.label()));
    }
}

#[derive(Clone, Copy)]
enum SuccNorm {
    SelfLoop,
    Raw,
}

#[derive(PartialEq, Eq, Hash)]
enum SuccKey {
    SelfLoop,
    Other(Label),
}

#[derive(PartialEq, Eq, Hash)]
enum StateKey {
    Match {
        nav: Nav,
        node_kind: NodeKindConstraint,
        node_field: Option<NodeFieldId>,
        missing: bool,
        effects: Vec<EffectIR>,
        neg_fields: Vec<NodeFieldId>,
        predicate: Option<PredicateIR>,
        successors: Vec<SuccKey>,
    },
    Call {
        entry: CallEntry,
        returns: Vec<SuccKey>,
        target: Label,
    },
}

impl StateKey {
    fn of(instr: &InstructionIR, norm: SuccNorm) -> Option<Self> {
        match instr {
            InstructionIR::Match(MatchIR {
                label,
                nav,
                node_kind,
                node_field,
                missing,
                effects,
                neg_fields,
                predicate,
                successors,
            }) => Some(Self::Match {
                nav: *nav,
                node_kind: *node_kind,
                node_field: *node_field,
                missing: *missing,
                effects: effects.clone(),
                neg_fields: neg_fields.clone(),
                predicate: predicate.clone(),
                successors: successors
                    .iter()
                    .map(|&s| match norm {
                        SuccNorm::SelfLoop if s == *label => SuccKey::SelfLoop,
                        _ => SuccKey::Other(s),
                    })
                    .collect(),
            }),
            InstructionIR::Call(c) => Some(Self::Call {
                entry: c.entry,
                returns: c
                    .returns
                    .iter()
                    .map(|&s| match norm {
                        SuccNorm::SelfLoop if s == c.label => SuccKey::SelfLoop,
                        _ => SuccKey::Other(s),
                    })
                    .collect(),
                target: c.target,
            }),
            InstructionIR::Return(_) => None,
        }
    }
}

fn plan_merges(instructions: &[InstructionIR], norm: SuccNorm) -> HashMap<Label, Label> {
    let mut representatives: HashMap<StateKey, Label> = HashMap::new();
    let mut remap: HashMap<Label, Label> = HashMap::new();

    for instr in instructions {
        let Some(key) = StateKey::of(instr, norm) else {
            continue;
        };
        match representatives.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(instr.label());
            }
            Entry::Occupied(rep) => {
                remap.insert(instr.label(), *rep.get());
            }
        }
    }

    remap
}

fn apply_remap(nfa: &mut NfaGraph, remap: &HashMap<Label, Label>) {
    if let Some((&label, &representative)) = remap
        .iter()
        .find(|(_, representative)| remap.contains_key(representative))
    {
        panic!(
            "NFA state deduplication produced a chained remap: {label:?} maps to \
             {representative:?}, which is itself remapped; remaps must point directly to a final \
             representative"
        );
    }
    let resolve = |label: Label| remap.get(&label).copied().unwrap_or(label);

    for instr in &mut nfa.instructions {
        match instr {
            InstructionIR::Match(m) => {
                for succ in &mut m.successors {
                    *succ = resolve(*succ);
                }
            }
            InstructionIR::Call(c) => {
                c.remap_returns(resolve);
                c.target = resolve(c.target);
            }
            InstructionIR::Return(_) => {}
        }
    }

    for entry in nfa.def_entries.values_mut() {
        *entry = resolve(*entry);
    }
    for entry in nfa.entrypoints.values_mut() {
        entry.target = resolve(entry.target);
    }
}

fn dedup_successors(instructions: &mut [InstructionIR]) {
    for instr in instructions {
        let InstructionIR::Match(m) = instr else {
            continue;
        };
        if m.successors.len() < 2 {
            continue;
        }
        let mut seen = std::collections::HashSet::with_capacity(m.successors.len());
        m.successors.retain(|&s| seen.insert(s));
    }
}
