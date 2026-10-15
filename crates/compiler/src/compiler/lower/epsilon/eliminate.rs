use std::collections::{HashMap, HashSet};

use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label, MatchIR, NfaGraph};

fn build_label_to_index(instructions: &[InstructionIR]) -> HashMap<Label, usize> {
    instructions
        .iter()
        .enumerate()
        .map(|(i, instr)| (instr.label(), i))
        .collect()
}

fn build_predecessor_map(instructions: &[InstructionIR]) -> HashMap<Label, Vec<Label>> {
    let mut preds: HashMap<Label, Vec<Label>> = HashMap::new();
    for instr in instructions {
        let from = instr.label();
        for &succ in instr.successors() {
            preds.entry(succ).or_default().push(from);
        }
    }
    preds
}

struct InstrIndex<'a> {
    instructions: &'a [InstructionIR],
    idx: &'a HashMap<Label, usize>,
}

impl<'a> InstrIndex<'a> {
    fn new(instructions: &'a [InstructionIR], idx: &'a HashMap<Label, usize>) -> Self {
        Self { instructions, idx }
    }

    fn match_at(&self, label: Label) -> Option<&'a MatchIR> {
        match &self.instructions[*self.idx.get(&label)?] {
            InstructionIR::Match(m) => Some(m),
            _ => None,
        }
    }

    fn see_through(&self, start: Label) -> Option<(Label, Vec<EffectIR>)> {
        let mut current = start;
        let mut effects = Vec::new();
        let mut visited = HashSet::new();

        loop {
            if !visited.insert(current) {
                return None;
            }

            let Some(m) = self.match_at(current) else {
                return Some((current, effects));
            };

            if !m.is_epsilon() {
                return Some((current, effects));
            }

            if m.successors.len() != 1 {
                return Some((current, effects));
            }

            if m.effects
                .iter()
                .any(|effect| effect.kind().is_motion_barrier())
            {
                return Some((current, effects));
            }

            effects.extend(m.effects.iter().cloned());
            current = m.successors[0];
        }
    }
}

struct InstrIndexMut<'a> {
    instructions: &'a mut [InstructionIR],
    idx: &'a HashMap<Label, usize>,
}

impl<'a> InstrIndexMut<'a> {
    fn new(instructions: &'a mut [InstructionIR], idx: &'a HashMap<Label, usize>) -> Self {
        Self { instructions, idx }
    }

    fn match_at_mut(&mut self, label: Label) -> Option<&mut MatchIR> {
        match &mut self.instructions[*self.idx.get(&label)?] {
            InstructionIR::Match(m) => Some(m),
            _ => None,
        }
    }
}

struct MatchEdit {
    successors: Vec<Label>,
    effects: Vec<EffectIR>,
}

impl MatchEdit {
    fn from_match(m: &MatchIR) -> Self {
        Self {
            successors: m.successors.clone(),
            effects: m.effects.clone(),
        }
    }

    fn rewrite_successor(&mut self, index: usize, target: Label, effects: Vec<EffectIR>) {
        self.successors[index] = target;
        self.effects.extend(effects);
    }

    fn apply_to(self, m: &mut MatchIR) {
        m.successors = self.successors;
        m.effects = self.effects;
    }
}

fn reads_cursor(effects: &[EffectIR]) -> bool {
    effects.iter().any(|effect| effect.kind().reads_cursor())
}

fn forward_migrate(instructions: &mut [InstructionIR]) -> bool {
    let mut changed = false;
    let preds = build_predecessor_map(instructions);
    let idx = build_label_to_index(instructions);

    for i in 0..instructions.len() {
        let eps = match &instructions[i] {
            InstructionIR::Match(m) if m.is_epsilon() => m,
            _ => continue,
        };

        if eps.effects.is_empty() {
            continue;
        }

        if eps
            .effects
            .iter()
            .any(|effect| effect.kind().is_motion_barrier())
        {
            continue;
        }

        if eps.successors.len() != 1 {
            continue;
        }

        let succ_label = eps.successors[0];

        let Some(succ) = InstrIndex::new(instructions, &idx).match_at(succ_label) else {
            continue;
        };
        if succ.is_epsilon() {
            continue;
        }

        if reads_cursor(&eps.effects) {
            continue;
        }

        let is_exclusive = preds
            .get(&succ_label)
            .is_some_and(|p| p.len() == 1 && p[0] == eps.label);
        if !is_exclusive {
            continue;
        }

        let eps_label = eps.label;
        let eps_effects = eps.effects.clone();

        let mut view = InstrIndexMut::new(instructions, &idx);

        let succ = view
            .match_at_mut(succ_label)
            .expect("succ_label resolved via match_at above, so it indexes a Match instruction");
        let mut effects = eps_effects;
        effects.append(&mut succ.effects);
        succ.effects = effects;

        let eps = view
            .match_at_mut(eps_label)
            .expect("eps_label is the current epsilon Match instruction at index i");
        eps.effects.clear();

        changed = true;
    }

    changed
}

fn laser_vision(result: &mut NfaGraph) -> bool {
    let mut changed = false;
    let idx = build_label_to_index(&result.instructions);

    let mut entry_remaps: HashMap<Label, Label> = HashMap::new();
    for entry in result.def_entries.values_mut() {
        if let Some((target, effects)) =
            InstrIndex::new(&result.instructions, &idx).see_through(*entry)
            && effects.is_empty()
            && target != *entry
        {
            entry_remaps.insert(*entry, target);
            *entry = target;
            changed = true;
        }
    }
    for instr in &mut result.instructions {
        if let InstructionIR::Call(c) = instr
            && let Some(&new) = entry_remaps.get(&c.target)
        {
            c.target = new;
        }
    }

    for entry in result.entrypoints.values_mut() {
        if let Some((target, effects)) =
            InstrIndex::new(&result.instructions, &idx).see_through(entry.target)
            && effects.is_empty()
            && target != entry.target
        {
            entry.target = target;
            changed = true;
        }
    }

    for i in 0..result.instructions.len() {
        let m = match &result.instructions[i] {
            InstructionIR::Match(m) => m,
            _ => continue,
        };

        let single = m.successors.len() == 1;
        let mut edited: Option<MatchEdit> = None;

        for (j, &succ) in m.successors.iter().enumerate() {
            let Some((target, effects)) =
                InstrIndex::new(&result.instructions, &idx).see_through(succ)
            else {
                continue;
            };

            if target == succ {
                continue;
            }

            if !effects.is_empty() && !single {
                continue;
            }

            edited
                .get_or_insert_with(|| MatchEdit::from_match(m))
                .rewrite_successor(j, target, effects);
        }

        if let Some(edit) = edited {
            let m = match &mut result.instructions[i] {
                InstructionIR::Match(m) => m,
                _ => unreachable!(),
            };
            edit.apply_to(m);
            changed = true;
        }
    }

    for i in 0..result.instructions.len() {
        let returns = match &result.instructions[i] {
            InstructionIR::Call(c) => c.return_labels().to_vec(),
            _ => continue,
        };
        let index = InstrIndex::new(&result.instructions, &idx);
        let remapped = returns
            .iter()
            .copied()
            .map(|next| {
                let Some((target, effects)) = index.see_through(next) else {
                    return next;
                };
                if effects.is_empty() { target } else { next }
            })
            .collect::<Vec<_>>();
        if remapped == returns {
            continue;
        }
        let InstructionIR::Call(call) = &mut result.instructions[i] else {
            unreachable!("selected a call instruction")
        };
        call.remap_returns(|label| {
            let index = returns
                .iter()
                .position(|&original| original == label)
                .expect("call return belongs to its original route set");
            remapped[index]
        });
        changed = true;
    }

    changed
}

fn expand_epsilon_forks(result: &mut NfaGraph) -> bool {
    let idx = build_label_to_index(&result.instructions);
    let preds = build_predecessor_map(&result.instructions);
    let mut changed = false;

    for i in 0..result.instructions.len() {
        let m = match &result.instructions[i] {
            InstructionIR::Match(m) => m,
            _ => continue,
        };

        if !m.is_epsilon() {
            continue;
        }
        if !m.effects.is_empty() {
            continue;
        }
        if m.successors.len() <= 1 {
            continue;
        }

        let eps_label = m.label;
        let eps_succs = m.successors.clone();

        if let Some(pred_labels) = preds.get(&eps_label) {
            for &pred_label in pred_labels {
                let pred_idx = idx[&pred_label];
                if let InstructionIR::Match(pred) = &mut result.instructions[pred_idx]
                    && let Some(pos) = pred.successors.iter().position(|&l| l == eps_label)
                {
                    pred.successors
                        .splice(pos..pos + 1, eps_succs.iter().cloned());
                    changed = true;
                }
            }
        }
    }

    changed
}

pub fn eliminate_epsilons(result: &mut NfaGraph) {
    loop {
        let a = forward_migrate(&mut result.instructions);
        let b = expand_epsilon_forks(result);
        let c = laser_vision(result);
        if !a && !b && !c {
            break;
        }
    }
}
