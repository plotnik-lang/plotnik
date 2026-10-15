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

    let mut predecessor_count: HashMap<Label, usize> = HashMap::new();
    for instr in &result.instructions {
        for &succ in instr.successors() {
            *predecessor_count.entry(succ).or_default() += 1;
        }
    }

    let mut removed: HashSet<Label> = HashSet::new();

    for i in 0..result.instructions.len() {
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

            let preds = predecessor_count
                .get(&succ_label)
                .copied()
                .expect("a label reached as a successor was counted as one");
            if preds != 1 {
                break;
            }

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

fn is_effectless(m: &MatchIR) -> bool {
    m.node_kind == NodeKindConstraint::Any
        && m.node_field.is_none()
        && m.effects.is_empty()
        && m.neg_fields.is_empty()
        && m.predicate.is_none()
}
