//! Epsilon elimination pass.
//!
//! Eliminates epsilon transitions (pure control flow) from IR while preserving semantics.
//! Uses a three-phase iterative approach:
//!
//! 1. **Forward migration**: Effectful epsilons push effects to exclusive successors
//! 2. **Expand branching**: Effectless branching epsilons expanded into predecessors
//! 3. **Laser vision**: Instructions look through epsilon chains, absorbing or bypassing
//!
//! Phases iterate until no changes occur.

use std::collections::{HashMap, HashSet};

use crate::bytecode::EffectKind;

use crate::compiler::lower::ir::{CompileResult, EffectIR, InstructionIR, Label, MatchIR};

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
struct InstrTable<'a> {
    instructions: &'a [InstructionIR],
    idx: &'a HashMap<Label, usize>,
}

impl<'a> InstrTable<'a> {
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
                return Some((current, effects)); // Non-Match target (Call/Return/Trampoline)
            };

            if !m.is_epsilon() {
                return Some((current, effects));
            }

            if m.successors.len() != 1 {
                return Some((current, effects)); // Branching epsilon: visible but can't see through
            }

            effects.extend(m.pre_effects.iter().cloned());
            effects.extend(m.post_effects.iter().cloned());
            current = m.successors[0];
        }
    }
}

struct InstrTableMut<'a> {
    instructions: &'a mut [InstructionIR],
    idx: &'a HashMap<Label, usize>,
}

impl<'a> InstrTableMut<'a> {
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

/// Whether any effect reads the VM's `matched_node` (`Node`). Such
/// effects are position-sensitive: their meaning depends on which node was
/// most recently matched, so they cannot be reordered across a navigation.
fn reads_matched_node(effects: &[EffectIR]) -> bool {
    effects.iter().any(|e| e.kind() == EffectKind::Node)
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

        if eps.pre_effects.is_empty() && eps.post_effects.is_empty() {
            continue;
        }

        if eps.successors.len() != 1 {
            continue;
        }

        let succ_label = eps.successors[0];

        let Some(succ) = InstrTable::new(instructions, &idx).match_at(succ_label) else {
            continue;
        };
        if succ.is_epsilon() {
            continue;
        }

        // Effects that read `matched_node` (Node) must not migrate forward:
        // the non-epsilon successor's navigation clears `matched_node` before the
        // migrated effects would run, so they'd capture the successor's node
        // instead of the inbound one (#383). Keep such epsilons in place.
        if reads_matched_node(&eps.pre_effects) || reads_matched_node(&eps.post_effects) {
            continue;
        }

        // This epsilon must be successor's ONLY predecessor (exclusive edge)
        let is_exclusive = preds
            .get(&succ_label)
            .is_some_and(|p| p.len() == 1 && p[0] == eps.label);
        if !is_exclusive {
            continue;
        }

        // Migrate: effects go to successor's pre_effects (in order: eps.pre, eps.post, succ.pre)
        let eps_label = eps.label;
        let eps_pre = eps.pre_effects.clone();
        let eps_post = eps.post_effects.clone();

        let mut view = InstrTableMut::new(instructions, &idx);

        let succ = view
            .match_at_mut(succ_label)
            .expect("succ_label resolved via match_at above, so it indexes a Match instruction");
        let mut new_pre = eps_pre;
        new_pre.extend(eps_post);
        new_pre.append(&mut succ.pre_effects);
        succ.pre_effects = new_pre;

        let eps = view
            .match_at_mut(eps_label)
            .expect("eps_label is the current epsilon Match instruction at index i");
        eps.pre_effects.clear();
        eps.post_effects.clear();

        changed = true;
    }

    changed
}

/// Phase B: Laser vision rewrites.
///
/// Each instruction looks through epsilon chains to find reachable targets:
/// - Single-successor instructions can absorb effects
/// - Multi-successor instructions follow effectless chains only
fn laser_vision(result: &mut CompileResult) -> bool {
    let mut changed = false;
    let idx = build_label_to_index(&result.instructions);

    // Track old→new entry remaps to fix up Call targets referencing them.
    let mut entry_remaps: HashMap<Label, Label> = HashMap::new();
    for entry in result.def_entries.values_mut() {
        if let Some((target, effects)) =
            InstrTable::new(&result.instructions, &idx).see_through(*entry)
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

    if let Some((target, effects)) =
        InstrTable::new(&result.instructions, &idx).see_through(result.preamble_entry)
        && effects.is_empty()
        && target != result.preamble_entry
    {
        result.preamble_entry = target;
        changed = true;
    }

    for i in 0..result.instructions.len() {
        let m = match &result.instructions[i] {
            InstructionIR::Match(m) if !m.is_epsilon() => m,
            _ => continue,
        };

        let single = m.successors.len() == 1;
        // Cloned lazily on the first rewrite; the common no-op case allocates nothing.
        let mut edited: Option<(Vec<Label>, Vec<EffectIR>)> = None;

        for (j, &succ) in m.successors.iter().enumerate() {
            let Some((target, effects)) =
                InstrTable::new(&result.instructions, &idx).see_through(succ)
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

            // No `reads_matched_node` guard is needed here (unlike forward_migrate):
            // `m` is non-epsilon so it sets `matched_node`, and the seen-through
            // chain is all epsilons (which preserve it), so absorbing into `post`
            // still reads `m`'s node — the node these effects saw at their original
            // position. forward_migrate is unsafe only because it pushes effects
            // *past* a navigation that clears `matched_node`.
            let (succs, post) =
                edited.get_or_insert_with(|| (m.successors.clone(), m.post_effects.clone()));
            succs[j] = target;
            post.extend(effects);
        }

        if let Some((succs, post)) = edited {
            let m = match &mut result.instructions[i] {
                InstructionIR::Match(m) => m,
                _ => unreachable!(),
            };
            m.successors = succs;
            m.post_effects = post;
            changed = true;
        }
    }

    for i in 0..result.instructions.len() {
        let next_label = match &result.instructions[i] {
            InstructionIR::Call(c) => Some(c.next),
            InstructionIR::Trampoline(t) => Some(t.next),
            _ => None,
        };

        let Some(next) = next_label else { continue };
        let Some((target, effects)) = InstrTable::new(&result.instructions, &idx).see_through(next)
        else {
            continue;
        };

        if effects.is_empty() && target != next {
            match &mut result.instructions[i] {
                InstructionIR::Call(c) => c.next = target,
                InstructionIR::Trampoline(t) => t.next = target,
                _ => {}
            }
            changed = true;
        }
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
fn expand_branching_epsilons(result: &mut CompileResult) -> bool {
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
        if !m.pre_effects.is_empty() || !m.post_effects.is_empty() {
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
                // Call/Trampoline have single `next` - can't expand branching into them
            }
        }
    }

    changed
}

/// Eliminate epsilon transitions from compiled IR.
///
/// Runs the migrate/expand/laser-vision phases to a fixed point. Semantic
/// preservation is asserted by the caller via `verify::run_verified` (debug only).
pub fn eliminate_epsilons(result: &mut CompileResult) {
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
mod tests {
    use super::*;
    use crate::bytecode::Nav;
    use crate::compiler::lower::ir::EffectIR;

    fn make_epsilon(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::terminal(Label(label))
                .nav(Nav::Epsilon)
                .successors(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_match(label: u32, nav: Nav, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::terminal(Label(label))
                .nav(nav)
                .successors(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_epsilon_with_pre(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::terminal(Label(label))
                .nav(Nav::Epsilon)
                .pre_effect(EffectIR::start_struct())
                .successors(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_epsilon_with_post(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::terminal(Label(label))
                .nav(Nav::Epsilon)
                .post_effect(EffectIR::end_struct())
                .successors(succs.into_iter().map(Label).collect()),
        )
    }

    #[test]
    fn see_through_effectless_chain() {
        // 0 (ε) → 1 (ε) → 2 (match)
        let instructions = vec![
            make_epsilon(0, vec![1]),
            make_epsilon(1, vec![2]),
            make_match(2, Nav::Down, vec![]),
        ];
        let idx = build_label_to_index(&instructions);

        let (target, effects) = InstrTable::new(&instructions, &idx)
            .see_through(Label(0))
            .unwrap();
        assert_eq!(target, Label(2));
        assert!(effects.is_empty());
    }

    #[test]
    fn see_through_with_effects() {
        // 0 (ε+Struct) → 1 (ε+EndStruct) → 2 (match)
        let instructions = vec![
            make_epsilon_with_pre(0, vec![1]),
            make_epsilon_with_post(1, vec![2]),
            make_match(2, Nav::Down, vec![]),
        ];
        let idx = build_label_to_index(&instructions);

        let (target, effects) = InstrTable::new(&instructions, &idx)
            .see_through(Label(0))
            .unwrap();
        assert_eq!(target, Label(2));
        assert_eq!(effects.len(), 2); // Struct from 0, EndStruct from 1
    }

    #[test]
    fn see_through_blocked_by_branch() {
        // 0 (ε) → 1 (ε, branching) → [2, 3]
        let instructions = vec![
            make_epsilon(0, vec![1]),
            make_epsilon(1, vec![2, 3]),
            make_match(2, Nav::Down, vec![]),
            make_match(3, Nav::Down, vec![]),
        ];
        let idx = build_label_to_index(&instructions);

        // Can see through 0 to 1, but 1 is branching
        let (target, effects) = InstrTable::new(&instructions, &idx)
            .see_through(Label(0))
            .unwrap();
        assert_eq!(target, Label(1)); // Stops at branching epsilon
        assert!(effects.is_empty());

        // Starting from branching epsilon returns itself
        let (target, effects) = InstrTable::new(&instructions, &idx)
            .see_through(Label(1))
            .unwrap();
        assert_eq!(target, Label(1));
        assert!(effects.is_empty());
    }

    #[test]
    fn forward_migrate_to_exclusive_successor() {
        // 0 (ε+Struct) → 1 (match), only 0 points to 1
        let mut instructions = vec![
            make_epsilon_with_pre(0, vec![1]),
            make_match(1, Nav::Down, vec![]),
        ];

        forward_migrate(&mut instructions);

        let eps = match &instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert!(eps.pre_effects.is_empty());

        let m1 = match &instructions[1] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m1.pre_effects.len(), 1);
    }

    #[test]
    fn forward_migrate_blocked_by_multi_pred() {
        // 0, 2 both point to 1 (match)
        // ε can't forward-migrate because 1 has multiple preds
        let mut instructions = vec![
            make_epsilon_with_pre(0, vec![1]),
            make_match(1, Nav::Down, vec![]),
            make_match(2, Nav::Down, vec![1]),
        ];

        forward_migrate(&mut instructions);

        // Effects NOT moved (1 has multiple predecessors)
        let eps = match &instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(eps.pre_effects.len(), 1); // Still has effect
    }

    #[test]
    fn laser_vision_single_succ_absorbs_effects() {
        // 0 (match, single succ) → 1 (ε+Struct) → 2 (match)
        let instructions = vec![
            make_match(0, Nav::Down, vec![1]),
            make_epsilon_with_pre(1, vec![2]),
            make_match(2, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        laser_vision(&mut result);

        // 0 absorbed effects and now points to 2
        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(2)]);
        assert_eq!(m0.post_effects.len(), 1);
    }

    #[test]
    fn laser_vision_multi_succ_effectless_only() {
        // 0 (match) → [1 (ε), 3]
        // 1 (ε+Struct) → 2
        let instructions = vec![
            make_match(0, Nav::Down, vec![1, 3]),
            make_epsilon_with_pre(1, vec![2]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        laser_vision(&mut result);

        // 0 can't absorb effects (multi-succ), so 1 stays
        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(1), Label(3)]);
        assert!(m0.post_effects.is_empty());
    }

    #[test]
    fn combined_forward_then_laser() {
        // The tricky case:
        // 0 (match) → [1 (ε+Struct), 3]
        // 1 → 2 (match), only 1 points to 2
        //
        // Phase A: 1 forward-migrates Struct to 2.pre, 1 becomes effectless
        // Phase B: 0 sees through 1 (now effectless) to 2
        let instructions = vec![
            make_match(0, Nav::Down, vec![1, 3]),
            make_epsilon_with_pre(1, vec![2]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        forward_migrate(&mut result.instructions);

        // 1 should now be effectless, 2 has the effect
        let eps = match &result.instructions[1] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert!(eps.pre_effects.is_empty());

        let m2 = match &result.instructions[2] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m2.pre_effects.len(), 1);

        laser_vision(&mut result);

        // 0 should now point directly to 2
        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(2), Label(3)]);
    }

    #[test]
    fn entry_point_resolution() {
        // Entry at 0 (ε) → 1 (ε) → 2 (match)
        let instructions = vec![
            make_epsilon(0, vec![1]),
            make_epsilon(1, vec![2]),
            make_match(2, Nav::Down, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: {
                let mut m = indexmap::IndexMap::new();
                m.insert(crate::compiler::ids::DefId::from_raw(0), Label(0));
                m
            },
            preamble_entry: Label(0),
        };

        laser_vision(&mut result);

        assert_eq!(
            result.def_entries[&crate::compiler::ids::DefId::from_raw(0)],
            Label(2)
        );
    }

    #[test]
    fn branching_epsilon_preserved_by_laser_vision() {
        // 0 (match) → 1 (ε, branching) → [2, 3]
        let instructions = vec![
            make_match(0, Nav::Down, vec![1]),
            make_epsilon(1, vec![2, 3]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        // laser_vision alone can't see through branching epsilon
        laser_vision(&mut result);

        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(1)]);
    }

    #[test]
    fn expand_branching_epsilon() {
        // 0 (match) → 1 (ε, branching) → [2, 3]
        // After expansion: 0 → [2, 3]
        let instructions = vec![
            make_match(0, Nav::Down, vec![1]),
            make_epsilon(1, vec![2, 3]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        expand_branching_epsilons(&mut result);

        // 0 now points directly to [2, 3]
        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(2), Label(3)]);
    }

    #[test]
    fn expand_branching_multiple_predecessors() {
        // Both 0 and 4 point to branching epsilon 1
        // 0 → 1 (ε) → [2, 3]
        // 4 → 1
        // After: 0 → [2, 3], 4 → [2, 3]
        let instructions = vec![
            make_match(0, Nav::Down, vec![1]),
            make_epsilon(1, vec![2, 3]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
            make_match(4, Nav::Down, vec![1]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        expand_branching_epsilons(&mut result);

        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(2), Label(3)]);

        let m4 = match &result.instructions[4] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m4.successors, vec![Label(2), Label(3)]);
    }

    #[test]
    fn expand_branching_preserves_other_successors() {
        // 0 → [1 (ε), 4]
        // 1 → [2, 3]
        // After: 0 → [2, 3, 4]
        let instructions = vec![
            make_match(0, Nav::Down, vec![1, 4]),
            make_epsilon(1, vec![2, 3]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
            make_match(4, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        expand_branching_epsilons(&mut result);

        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(2), Label(3), Label(4)]);
    }

    #[test]
    fn expand_blocked_by_effects() {
        // 0 → 1 (ε+Obj, branching) → [2, 3]
        // Effectful branching epsilon cannot be expanded
        let instructions = vec![
            make_match(0, Nav::Down, vec![1]),
            make_epsilon_with_pre(1, vec![2, 3]),
            make_match(2, Nav::Next, vec![]),
            make_match(3, Nav::Next, vec![]),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: indexmap::IndexMap::new(),
            preamble_entry: Label(0),
        };

        let changed = expand_branching_epsilons(&mut result);
        assert!(!changed);

        // 0 still points to 1
        let m0 = match &result.instructions[0] {
            InstructionIR::Match(m) => m,
            _ => panic!(),
        };
        assert_eq!(m0.successors, vec![Label(1)]);
    }
}
