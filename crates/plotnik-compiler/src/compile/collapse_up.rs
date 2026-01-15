//! Up-collapse optimization: merge consecutive Up instructions of the same mode.
//!
//! Transforms: Up(1) → Up(1) → Up(2) into Up(4)
//!
//! Constraints:
//! - Same mode only (Up, UpSkipTrivia, UpExact can't mix)
//! - Effectless only (no pre_effects, post_effects, neg_fields)
//! - Max 63 (6-bit payload limit)
//! - Single successor (can't merge branching instructions)

use std::collections::{HashMap, HashSet};

use plotnik_bytecode::Nav;

use crate::bytecode::{InstructionIR, Label, MatchIR, NodeTypeIR};
use crate::compile::CompileResult;

const MAX_UP_LEVEL: u8 = 63;

/// Collapse consecutive Up instructions of the same mode.
pub fn collapse_up(result: &mut CompileResult) {
    let label_to_idx: HashMap<Label, usize> = result
        .instructions
        .iter()
        .enumerate()
        .map(|(i, instr)| (instr.label(), i))
        .collect();

    // Count predecessors for each label - only remove labels with exactly one predecessor
    let mut predecessor_count: HashMap<Label, usize> = HashMap::new();
    for instr in &result.instructions {
        for succ in instr.successors() {
            *predecessor_count.entry(succ).or_default() += 1;
        }
    }

    let mut removed: HashSet<Label> = HashSet::new();

    for i in 0..result.instructions.len() {
        let InstructionIR::Match(m) = &result.instructions[i] else {
            continue;
        };

        let Some(up_level) = get_up_level(m.nav) else {
            continue;
        };

        if m.successors.len() != 1 {
            continue;
        }

        let mut current_level = up_level;
        let mut current_nav = m.nav;
        let mut final_successors = m.successors.clone();

        // Absorb chain of effectless Up instructions with same mode
        while current_level < MAX_UP_LEVEL {
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

            let Some(succ_level) = get_up_level(succ.nav) else {
                break;
            };

            if !same_up_mode(current_nav, succ.nav) || !is_effectless(succ) {
                break;
            }

            // Only absorb if this label has exactly one predecessor
            // (otherwise other instructions still need it)
            if predecessor_count.get(&succ_label).copied().unwrap_or(0) != 1 {
                break;
            }

            // Merge: add levels (capped at 63)
            let new_level = current_level.saturating_add(succ_level).min(MAX_UP_LEVEL);
            current_nav = set_up_level(current_nav, new_level);
            current_level = new_level;
            final_successors = succ.successors.clone();
            removed.insert(succ_label);
        }

        // Update the instruction if we merged anything
        if current_level != up_level {
            let InstructionIR::Match(m) = &mut result.instructions[i] else {
                unreachable!()
            };
            m.nav = current_nav;
            m.successors = final_successors;
        }
    }

    // Remove absorbed instructions
    result
        .instructions
        .retain(|instr| !removed.contains(&instr.label()));
}

/// Extract Up level from Nav, if it's an Up variant.
fn get_up_level(nav: Nav) -> Option<u8> {
    match nav {
        Nav::Up(n) | Nav::UpSkipTrivia(n) | Nav::UpExact(n) => Some(n),
        _ => None,
    }
}

/// Set the level on an Up Nav variant.
fn set_up_level(nav: Nav, level: u8) -> Nav {
    match nav {
        Nav::Up(_) => Nav::Up(level),
        Nav::UpSkipTrivia(_) => Nav::UpSkipTrivia(level),
        Nav::UpExact(_) => Nav::UpExact(level),
        _ => nav,
    }
}

/// Check if two Nav values are the same Up mode (ignoring level).
fn same_up_mode(a: Nav, b: Nav) -> bool {
    matches!(
        (a, b),
        (Nav::Up(_), Nav::Up(_))
            | (Nav::UpSkipTrivia(_), Nav::UpSkipTrivia(_))
            | (Nav::UpExact(_), Nav::UpExact(_))
    )
}

/// Check if a MatchIR has no effects or constraints (pure navigation).
fn is_effectless(m: &MatchIR) -> bool {
    m.node_type == NodeTypeIR::Any
        && m.node_field.is_none()
        && m.pre_effects.is_empty()
        && m.neg_fields.is_empty()
        && m.post_effects.is_empty()
        && m.predicate.is_none()
}
