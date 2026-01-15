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

use crate::bytecode::{EffectIR, InstructionIR, Label, MatchIR};
use crate::compile::error::CompileResult;

/// Build label → index map for quick instruction lookup.
fn build_label_to_index(instructions: &[InstructionIR]) -> HashMap<Label, usize> {
    instructions
        .iter()
        .enumerate()
        .map(|(i, instr)| (instr.label(), i))
        .collect()
}

/// Build predecessor map: label → labels that transition to it.
fn build_predecessor_map(instructions: &[InstructionIR]) -> HashMap<Label, Vec<Label>> {
    let mut preds: HashMap<Label, Vec<Label>> = HashMap::new();
    for instr in instructions {
        let from = instr.label();
        for succ in instr.successors() {
            preds.entry(succ).or_default().push(from);
        }
    }
    preds
}

/// Get a Match instruction by label.
fn get_match<'a>(
    label: Label,
    instructions: &'a [InstructionIR],
    idx: &HashMap<Label, usize>,
) -> Option<&'a MatchIR> {
    match &instructions[*idx.get(&label)?] {
        InstructionIR::Match(m) => Some(m),
        _ => None,
    }
}

/// Get a mutable Match instruction by label.
fn get_match_mut<'a>(
    label: Label,
    instructions: &'a mut [InstructionIR],
    idx: &HashMap<Label, usize>,
) -> Option<&'a mut MatchIR> {
    match &mut instructions[*idx.get(&label)?] {
        InstructionIR::Match(m) => Some(m),
        _ => None,
    }
}

/// See through single-successor epsilon chains.
///
/// Returns `(target, accumulated_effects)` or `None` if blocked by:
/// - Branching epsilon (multiple successors)
/// - Cycle
fn see_through(
    start: Label,
    instructions: &[InstructionIR],
    idx: &HashMap<Label, usize>,
) -> Option<(Label, Vec<EffectIR>)> {
    let mut current = start;
    let mut effects = Vec::new();
    let mut visited = HashSet::new();

    loop {
        if !visited.insert(current) {
            return None; // Cycle
        }

        let Some(m) = get_match(current, instructions, idx) else {
            return Some((current, effects)); // Non-Match target (Call/Return/Trampoline)
        };

        if !m.is_epsilon() {
            return Some((current, effects)); // Visible: non-epsilon Match
        }

        if m.successors.len() != 1 {
            return Some((current, effects)); // Branching epsilon: visible but can't see through
        }

        // Single-succ epsilon: absorb effects, continue looking
        effects.extend(m.pre_effects.iter().cloned());
        effects.extend(m.post_effects.iter().cloned());
        current = m.successors[0];
    }
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

        // Skip effectless epsilons
        if eps.pre_effects.is_empty() && eps.post_effects.is_empty() {
            continue;
        }

        // Must have single successor
        if eps.successors.len() != 1 {
            continue;
        }

        let succ_label = eps.successors[0];

        // Successor must be a non-epsilon Match
        let Some(succ) = get_match(succ_label, instructions, &idx) else {
            continue;
        };
        if succ.is_epsilon() {
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

        let succ = get_match_mut(succ_label, instructions, &idx).unwrap();
        let mut new_pre = eps_pre;
        new_pre.extend(eps_post);
        new_pre.append(&mut succ.pre_effects);
        succ.pre_effects = new_pre;

        let eps = get_match_mut(eps_label, instructions, &idx).unwrap();
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

    // Entry points: resolve through effectless chains
    // Track old->new mappings to update Call targets
    let mut entry_remaps: HashMap<Label, Label> = HashMap::new();
    for entry in result.def_entries.values_mut() {
        if let Some((target, effects)) = see_through(*entry, &result.instructions, &idx)
            && effects.is_empty()
            && target != *entry
        {
            entry_remaps.insert(*entry, target);
            *entry = target;
            changed = true;
        }
    }

    // Update Call targets to match updated def_entries
    for instr in &mut result.instructions {
        if let InstructionIR::Call(c) = instr
            && let Some(&new) = entry_remaps.get(&c.target)
        {
            c.target = new;
        }
    }

    // Preamble entry
    if let Some((target, effects)) = see_through(result.preamble_entry, &result.instructions, &idx)
        && effects.is_empty()
        && target != result.preamble_entry
    {
        result.preamble_entry = target;
        changed = true;
    }

    // Non-epsilon Match instructions: resolve successors
    for i in 0..result.instructions.len() {
        let m = match &result.instructions[i] {
            InstructionIR::Match(m) if !m.is_epsilon() => m,
            _ => continue,
        };

        let single = m.successors.len() == 1;
        let mut succs = m.successors.clone();
        let mut post = m.post_effects.clone();
        let mut modified = false;

        for (j, &succ) in m.successors.iter().enumerate() {
            let Some((target, effects)) = see_through(succ, &result.instructions, &idx) else {
                continue;
            };

            if target == succ {
                continue; // Nothing to see through
            }

            // Effects require single successor (can't execute for all paths)
            if !effects.is_empty() && !single {
                continue;
            }

            succs[j] = target;
            post.extend(effects);
            modified = true;
        }

        if modified {
            let m = match &mut result.instructions[i] {
                InstructionIR::Match(m) => m,
                _ => unreachable!(),
            };
            m.successors = succs;
            m.post_effects = post;
            changed = true;
        }
    }

    // Call/Trampoline: resolve next (effectless only)
    for i in 0..result.instructions.len() {
        let next_label = match &result.instructions[i] {
            InstructionIR::Call(c) => Some(c.next),
            InstructionIR::Trampoline(t) => Some(t.next),
            _ => None,
        };

        let Some(next) = next_label else { continue };
        let Some((target, effects)) = see_through(next, &result.instructions, &idx) else {
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

        // Must be effectless epsilon with multiple successors
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

        // Expand into each predecessor
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
/// In debug builds, verifies semantic fingerprints are preserved.
#[allow(unused_variables)]
pub fn eliminate_epsilons(result: &mut CompileResult, ctx: &super::compiler::CompileCtx) {
    #[cfg(debug_assertions)]
    let before: Vec<_> = {
        use super::verify::fingerprint_from_ir;
        result
            .def_entries
            .iter()
            .map(|(def_id, &entry)| {
                let fp = fingerprint_from_ir(&result.instructions, entry, &result.def_entries, ctx);
                (*def_id, fp)
            })
            .collect()
    };

    // Iterate phases until fixed point
    loop {
        let a = forward_migrate(&mut result.instructions);
        let b = expand_branching_epsilons(result);
        let c = laser_vision(result);
        if !a && !b && !c {
            break;
        }
    }

    #[cfg(debug_assertions)]
    {
        use super::verify::fingerprint_from_ir;
        for (def_id, before_fp) in before {
            let entry = result.def_entries[&def_id];
            let after_fp =
                fingerprint_from_ir(&result.instructions, entry, &result.def_entries, ctx);

            if before_fp != after_fp {
                eprintln!("=== Fingerprint mismatch for def {:?} ===", def_id);
                eprintln!("Before ({} paths):", before_fp.len());
                for (i, path) in before_fp.iter().enumerate() {
                    eprintln!("  {}: {:?}", i, path);
                }
                eprintln!("After ({} paths):", after_fp.len());
                for (i, path) in after_fp.iter().enumerate() {
                    eprintln!("  {}: {:?}", i, path);
                }
                panic!("epsilon elimination changed semantics for def {:?}", def_id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::EffectIR;
    use plotnik_bytecode::Nav;

    fn make_epsilon(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::at(Label(label))
                .nav(Nav::Epsilon)
                .next_many(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_match(label: u32, nav: Nav, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::at(Label(label))
                .nav(nav)
                .next_many(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_epsilon_with_pre(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::at(Label(label))
                .nav(Nav::Epsilon)
                .pre_effect(EffectIR::start_obj())
                .next_many(succs.into_iter().map(Label).collect()),
        )
    }

    fn make_epsilon_with_post(label: u32, succs: Vec<u32>) -> InstructionIR {
        InstructionIR::Match(
            MatchIR::at(Label(label))
                .nav(Nav::Epsilon)
                .post_effect(EffectIR::end_obj())
                .next_many(succs.into_iter().map(Label).collect()),
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

        let (target, effects) = see_through(Label(0), &instructions, &idx).unwrap();
        assert_eq!(target, Label(2));
        assert!(effects.is_empty());
    }

    #[test]
    fn see_through_with_effects() {
        // 0 (ε+Obj) → 1 (ε+EndObj) → 2 (match)
        let instructions = vec![
            make_epsilon_with_pre(0, vec![1]),
            make_epsilon_with_post(1, vec![2]),
            make_match(2, Nav::Down, vec![]),
        ];
        let idx = build_label_to_index(&instructions);

        let (target, effects) = see_through(Label(0), &instructions, &idx).unwrap();
        assert_eq!(target, Label(2));
        assert_eq!(effects.len(), 2); // Obj from 0, EndObj from 1
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
        let (target, effects) = see_through(Label(0), &instructions, &idx).unwrap();
        assert_eq!(target, Label(1)); // Stops at branching epsilon
        assert!(effects.is_empty());

        // Starting from branching epsilon returns itself
        let (target, effects) = see_through(Label(1), &instructions, &idx).unwrap();
        assert_eq!(target, Label(1));
        assert!(effects.is_empty());
    }

    #[test]
    fn forward_migrate_to_exclusive_successor() {
        // 0 (ε+Obj) → 1 (match), only 0 points to 1
        let mut instructions = vec![
            make_epsilon_with_pre(0, vec![1]),
            make_match(1, Nav::Down, vec![]),
        ];

        forward_migrate(&mut instructions);

        // Effects moved to 1.pre
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
        // 0 (match, single succ) → 1 (ε+Obj) → 2 (match)
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
        // 1 (ε+Obj) → 2
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
        // 0 (match) → [1 (ε+Obj), 3]
        // 1 → 2 (match), only 1 points to 2
        //
        // Phase A: 1 forward-migrates Obj to 2.pre, 1 becomes effectless
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

        // Phase A
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

        // Phase B
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
                m.insert(crate::analyze::type_check::DefId::from_raw(0), Label(0));
                m
            },
            preamble_entry: Label(0),
        };

        laser_vision(&mut result);

        assert_eq!(
            result.def_entries[&crate::analyze::type_check::DefId::from_raw(0)],
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
