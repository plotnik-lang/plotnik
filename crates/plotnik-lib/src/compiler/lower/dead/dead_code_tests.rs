use super::*;
use crate::bytecode::Nav;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{CallIR, CalleeEntry, DefVariant, MatchIR, ReturnAddr, ReturnIR};
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
            m.insert(DefVariant::ordinary(DefId::from_raw(0)), Label(0));
            m
        },
        entry_point_wrappers: IndexMap::from([(DefId::from_raw(0), Label(0))]),
        spans: None,
        label_origins: Vec::new(),
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
            m.insert(DefVariant::ordinary(DefId::from_raw(0)), Label(0));
            m
        },
        entry_point_wrappers: IndexMap::from([(DefId::from_raw(0), Label(0))]),
        spans: None,
        label_origins: Vec::new(),
    };

    remove_unreachable(&mut result);

    assert_eq!(result.instructions.len(), 3);
}

#[test]
fn handles_fork() {
    // A -> [B, C] (all reachable through the fork)
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
            m.insert(DefVariant::ordinary(DefId::from_raw(0)), Label(0));
            m
        },
        entry_point_wrappers: IndexMap::from([(DefId::from_raw(0), Label(0))]),
        spans: None,
        label_origins: Vec::new(),
    };

    remove_unreachable(&mut result);

    assert_eq!(result.instructions.len(), 3);
}

#[test]
fn follows_call_targets_and_prunes_unreachable_definition_entries() {
    let used = DefVariant::ordinary(DefId::from_raw(0));
    let unused = DefVariant::ordinary(DefId::from_raw(1));
    let instructions = vec![
        MatchIR::epsilon(Label(0), Label(1)).into(),
        CallIR::new(Label(1), ReturnAddr(Label(2)), CalleeEntry(Label(3))).into(),
        MatchIR::terminal(Label(2)).into(),
        MatchIR::epsilon(Label(3), Label(4)).into(),
        ReturnIR::new(Label(4)).into(),
        ReturnIR::new(Label(5)).into(),
    ];
    let mut result = NfaGraph {
        instructions,
        def_entries: IndexMap::from([(used.clone(), Label(3)), (unused, Label(5))]),
        entry_point_wrappers: IndexMap::from([(DefId::from_raw(2), Label(0))]),
        spans: None,
        label_origins: Vec::new(),
    };

    remove_unreachable(&mut result);

    assert!(result.instructions.iter().any(|i| i.label() == Label(3)));
    assert!(result.instructions.iter().any(|i| i.label() == Label(4)));
    assert!(!result.instructions.iter().any(|i| i.label() == Label(5)));
    assert_eq!(result.def_entries, IndexMap::from([(used, Label(3))]));
}

#[test]
fn keeps_reachable_recursive_definition() {
    let recursive = DefVariant::ordinary(DefId::from_raw(0));
    let instructions = vec![
        MatchIR::epsilon(Label(0), Label(1)).into(),
        CallIR::new(Label(1), ReturnAddr(Label(2)), CalleeEntry(Label(3))).into(),
        MatchIR::terminal(Label(2)).into(),
        MatchIR::terminal(Label(3))
            .successors(vec![Label(4), Label(5)])
            .into(),
        CallIR::new(Label(4), ReturnAddr(Label(6)), CalleeEntry(Label(3))).into(),
        ReturnIR::new(Label(5)).into(),
        ReturnIR::new(Label(6)).into(),
    ];
    let mut result = NfaGraph {
        instructions,
        def_entries: IndexMap::from([(recursive.clone(), Label(3))]),
        entry_point_wrappers: IndexMap::from([(DefId::from_raw(1), Label(0))]),
        spans: None,
        label_origins: Vec::new(),
    };

    remove_unreachable(&mut result);

    assert_eq!(result.instructions.len(), 7);
    assert_eq!(result.def_entries, IndexMap::from([(recursive, Label(3))]));
}
