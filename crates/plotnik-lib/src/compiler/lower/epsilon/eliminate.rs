//! Epsilon elimination pass.
//!
//! Eliminates epsilon transitions (pure control flow) from IR while preserving semantics.
//! Uses a three-phase iterative approach:
//!
//! 1. **Forward migration**: Effectful epsilons push effects to exclusive successors
//! 2. **Expand branching**: Effectless branching epsilons expanded into predecessors
//! 3. **Laser vision**: Every instruction (epsilons included) looks through epsilon
//!    chains, absorbing or bypassing
//!
//! Phases iterate until no changes occur.

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

/// An immutable view over the instruction list paired with its label→index map.
///
/// Bundles the `(instructions, idx)` clump every lookup threads, so a label is
/// always resolved against the index built for that exact list.
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

    /// See through single-successor epsilon chains.
    ///
    /// Returns `(target, accumulated_effects)` or `None` if blocked by:
    /// - Branching epsilon (multiple successors)
    /// - Cycle
    fn see_through(&self, start: Label) -> Option<(Label, Vec<EffectIR>)> {
        let mut current = start;
        let mut effects = Vec::new();
        let mut visited = HashSet::new();

        loop {
            if !visited.insert(current) {
                return None;
            }

            let Some(m) = self.match_at(current) else {
                return Some((current, effects)); // Non-Match target (Call/Return)
            };

            if !m.is_epsilon() {
                return Some((current, effects));
            }

            if m.successors.len() != 1 {
                return Some((current, effects)); // Branching epsilon: visible but can't see through
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

/// Whether any effect reads the VM cursor. Such effects are
/// position-sensitive: their meaning depends on the current cursor node, so
/// they cannot be reordered across a navigation.
fn reads_cursor(effects: &[EffectIR]) -> bool {
    effects.iter().any(|effect| effect.kind().reads_cursor())
}

/// Phase A: Forward migration.
///
/// Effectful epsilons with exclusive edge to a non-epsilon successor
/// push their effects forward, becoming effectless.
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

        // Effects that read the cursor (Node) must not migrate forward across
        // the non-epsilon successor's navigation: they would capture the
        // successor's node instead of the inbound one.
        if reads_cursor(&eps.effects) {
            continue;
        }

        // This epsilon must be successor's ONLY predecessor (exclusive edge)
        let is_exclusive = preds
            .get(&succ_label)
            .is_some_and(|p| p.len() == 1 && p[0] == eps.label);
        if !is_exclusive {
            continue;
        }

        // Migrate: epsilon effects run before the successor's own effects.
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

/// Phase B: Laser vision rewrites.
///
/// Each instruction looks through epsilon chains to find reachable targets:
/// - Single-successor instructions can absorb effects
/// - Multi-successor instructions follow effectless chains only
///
/// Epsilons participate as sources too: a single-successor epsilon absorbs the
/// whole downstream chain into itself (coalescing the effect chains that scope
/// brackets around a `Call` produce, since `CallIR` can't carry effects), and a
/// branching epsilon bypasses pure jumps per successor. Effects bubble *up*
/// toward branch points this way, while forward migration pushes them *down*
/// into non-epsilon instructions; both strictly shorten chains, so the
/// fixpoint terminates.
fn laser_vision(result: &mut NfaGraph) -> bool {
    let mut changed = false;
    let idx = build_label_to_index(&result.instructions);

    // Track old→new entry remaps to fix up Call targets referencing them.
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

    for entry in result.entrypoint_wrappers.values_mut() {
        if let Some((target, effects)) =
            InstrIndex::new(&result.instructions, &idx).see_through(*entry)
            && effects.is_empty()
            && target != *entry
        {
            *entry = target;
            changed = true;
        }
    }

    for i in 0..result.instructions.len() {
        let m = match &result.instructions[i] {
            InstructionIR::Match(m) => m,
            _ => continue,
        };

        let single = m.successors.len() == 1;
        // Cloned lazily on the first rewrite; the common no-op case allocates nothing.
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

            // Effects require single successor (can't execute for all paths)
            if !effects.is_empty() && !single {
                continue;
            }

            // No `reads_cursor` guard is needed here (unlike forward_migrate):
            // the seen-through chain is all epsilons, which preserve the cursor,
            // so appending still reads the node these effects saw at their
            // original position. forward_migrate is unsafe only because it
            // pushes effects past a navigation.
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

/// Phase C: Expand branching epsilons.
///
/// Effectless branching epsilons are expanded by replacing the epsilon
/// reference in each predecessor with the epsilon's successors.
///
/// Before:  a → [ε, x], ε → [d, e, f]
/// After:   a → [d, e, f, x]
///
/// The epsilon becomes unreachable and is eliminated during layout.
fn expand_branching_epsilons(result: &mut NfaGraph) -> bool {
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
            continue; // Single-succ handled by laser_vision
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
                // Call has a single `next` - can't expand branching into it
            }
        }
    }

    changed
}

/// Eliminate epsilon transitions from compiled IR.
///
/// Runs the migrate/expand/laser-vision phases to a fixed point. Semantic
/// preservation is asserted by the caller via `verify::run_verified` (debug only).
pub fn eliminate_epsilons(result: &mut NfaGraph) {
    loop {
        let a = forward_migrate(&mut result.instructions);
        let b = expand_branching_epsilons(result);
        let c = laser_vision(result);
        if !a && !b && !c {
            break;
        }
    }
}

#[cfg(test)]
#[path = "eliminate_tests.rs"]
mod eliminate_tests;
