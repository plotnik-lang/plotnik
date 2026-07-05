use super::*;
use crate::bytecode::Nav;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::MatchIR;
use indexmap::IndexMap;

#[test]
fn removes_unreachable_instructions() {
    // A -> B (reachable), C (unreachable)
    let instructions = vec![
        MatchIR::terminal(Label(0))
            .nav(Nav::Down)
            .next(Label(1))
            .into(),
        MatchIR::terminal(Label(1)).nav(Nav::Down).into(),
        MatchIR::terminal(Label(2)).nav(Nav::Down).into(), // unreachable
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: {
            let mut m = IndexMap::new();
            m.insert(DefId::from_raw(0), Label(0));
            m
        },
        def_entries_consuming: Default::default(),
        entrypoint_wrappers: Default::default(),
        spans: None,
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
        MatchIR::terminal(Label(0))
            .nav(Nav::Down)
            .next(Label(1))
            .into(),
        MatchIR::terminal(Label(1))
            .nav(Nav::Down)
            .next(Label(2))
            .into(),
        MatchIR::terminal(Label(2)).nav(Nav::Down).into(),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: {
            let mut m = IndexMap::new();
            m.insert(DefId::from_raw(0), Label(0));
            m
        },
        def_entries_consuming: Default::default(),
        entrypoint_wrappers: Default::default(),
        spans: None,
    };

    remove_unreachable(&mut result);

    assert_eq!(result.instructions.len(), 3);
}

#[test]
fn handles_branching() {
    // A -> [B, C] (all reachable via branch)
    let instructions = vec![
        MatchIR::terminal(Label(0))
            .nav(Nav::Down)
            .successors(vec![Label(1), Label(2)])
            .into(),
        MatchIR::terminal(Label(1)).nav(Nav::Down).into(),
        MatchIR::terminal(Label(2)).nav(Nav::Down).into(),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: {
            let mut m = IndexMap::new();
            m.insert(DefId::from_raw(0), Label(0));
            m
        },
        def_entries_consuming: Default::default(),
        entrypoint_wrappers: Default::default(),
        spans: None,
    };

    remove_unreachable(&mut result);

    assert_eq!(result.instructions.len(), 3);
}
