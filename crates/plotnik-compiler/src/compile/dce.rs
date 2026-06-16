//! Dead code elimination pass.
//!
//! Removes unreachable instructions after epsilon elimination.
//! Instructions become unreachable when epsilon transitions are
//! bypassed and no other path leads to them.

use std::collections::HashSet;

use crate::bytecode::Label;

use super::error::CompileResult;

pub fn remove_unreachable(result: &mut CompileResult) {
    let reachable = compute_reachable(result);
    result
        .instructions
        .retain(|instr| reachable.contains(&instr.label()));
}

fn compute_reachable(result: &CompileResult) -> HashSet<Label> {
    let successors: std::collections::BTreeMap<Label, Vec<Label>> = result
        .instructions
        .iter()
        .map(|instr| (instr.label(), instr.successors().to_vec()))
        .collect();

    let mut reachable = HashSet::new();
    let mut queue: Vec<Label> = vec![result.preamble_entry];
    queue.extend(result.def_entries.values().copied());

    while let Some(label) = queue.pop() {
        if !reachable.insert(label) {
            continue;
        }
        if let Some(succs) = successors.get(&label) {
            queue.extend(succs.iter().copied());
        }
    }

    reachable
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyze::type_check::DefId;
    use crate::bytecode::MatchIR;
    use indexmap::IndexMap;
    use plotnik_bytecode::Nav;

    #[test]
    fn removes_unreachable_instructions() {
        // A -> B (reachable), C (unreachable)
        let instructions = vec![
            MatchIR::at(Label(0)).nav(Nav::Down).next(Label(1)).into(),
            MatchIR::terminal(Label(1)).nav(Nav::Down).into(),
            MatchIR::terminal(Label(2)).nav(Nav::Down).into(), // unreachable
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: {
                let mut m = IndexMap::new();
                m.insert(DefId::from_raw(0), Label(0));
                m
            },
            preamble_entry: Label(0),
        };

        remove_unreachable(&mut result);

        assert_eq!(result.instructions.len(), 2);
        assert!(result.instructions.iter().any(|i| i.label() == Label(0)));
        assert!(result.instructions.iter().any(|i| i.label() == Label(1)));
        assert!(!result.instructions.iter().any(|i| i.label() == Label(2)));
    }

    #[test]
    fn keeps_all_when_all_reachable() {
        // A -> B -> C (all reachable)
        let instructions = vec![
            MatchIR::at(Label(0)).nav(Nav::Down).next(Label(1)).into(),
            MatchIR::at(Label(1)).nav(Nav::Down).next(Label(2)).into(),
            MatchIR::terminal(Label(2)).nav(Nav::Down).into(),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: {
                let mut m = IndexMap::new();
                m.insert(DefId::from_raw(0), Label(0));
                m
            },
            preamble_entry: Label(0),
        };

        remove_unreachable(&mut result);

        assert_eq!(result.instructions.len(), 3);
    }

    #[test]
    fn handles_branching() {
        // A -> [B, C] (all reachable via branch)
        let instructions = vec![
            MatchIR::at(Label(0))
                .nav(Nav::Down)
                .next_many(vec![Label(1), Label(2)])
                .into(),
            MatchIR::terminal(Label(1)).nav(Nav::Down).into(),
            MatchIR::terminal(Label(2)).nav(Nav::Down).into(),
        ];

        let mut result = CompileResult {
            instructions,
            def_entries: {
                let mut m = IndexMap::new();
                m.insert(DefId::from_raw(0), Label(0));
                m
            },
            preamble_entry: Label(0),
        };

        remove_unreachable(&mut result);

        assert_eq!(result.instructions.len(), 3);
    }
}
