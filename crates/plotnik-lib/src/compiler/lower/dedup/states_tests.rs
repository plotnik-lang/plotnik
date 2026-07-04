use super::*;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{CallIR, CalleeEntry, MatchIR, ReturnAddr, ReturnIR};
use indexmap::IndexMap;

fn graph(instructions: Vec<InstructionIR>, entry: u32) -> NfaGraph {
    NfaGraph {
        instructions,
        def_entries: {
            let mut m = IndexMap::new();
            m.insert(DefId::from_raw(0), Label(entry));
            m
        },
        preamble_entry: Label(entry),
    }
}

fn eps(label: u32, succs: Vec<u32>) -> InstructionIR {
    MatchIR::terminal(Label(label))
        .successors(succs.into_iter().map(Label).collect())
        .into()
}

fn nav_next(label: u32, succs: Vec<u32>) -> InstructionIR {
    MatchIR::terminal(Label(label))
        .nav(Nav::Next)
        .successors(succs.into_iter().map(Label).collect())
        .into()
}

fn labels(nfa: &NfaGraph) -> Vec<u32> {
    nfa.instructions.iter().map(|i| i.label().0).collect()
}

fn match_at(nfa: &NfaGraph, label: u32) -> &MatchIR {
    nfa.instructions
        .iter()
        .find_map(|i| match i {
            InstructionIR::Match(m) if m.label == Label(label) => Some(m),
            _ => None,
        })
        .expect("match instruction present")
}

#[test]
fn merges_identical_nav_twins() {
    // The position-search shape: `navigate` and `retry` are byte-identical
    // wildcard Next steps into the same `try` state.
    let mut nfa = graph(
        vec![
            eps(0, vec![1, 2]),   // try → [body, retry]
            eps(1, vec![]),       // body
            nav_next(2, vec![0]), // retry
            nav_next(3, vec![0]), // navigate (duplicate of retry)
        ],
        3,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 1, 2]);
    assert_eq!(nfa.def_entries[&DefId::from_raw(0)], Label(2));
    assert_eq!(nfa.preamble_entry, Label(2));
}

#[test]
fn merge_cascades_to_predecessors() {
    // A1 → B1 → T and A2 → B2 → T: the B's merge first, which makes the A's
    // identical in the next round.
    let mut nfa = graph(
        vec![
            eps(0, vec![10, 20]),
            MatchIR::terminal(Label(10))
                .nav(Nav::Down)
                .next(Label(11))
                .into(),
            nav_next(11, vec![1]),
            MatchIR::terminal(Label(20))
                .nav(Nav::Down)
                .next(Label(21))
                .into(),
            nav_next(21, vec![1]),
            eps(1, vec![]),
        ],
        0,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 10, 11, 1]);
    assert_eq!(match_at(&nfa, 0).successors, vec![Label(10)]);
}

#[test]
fn self_loop_twins_and_loop_jumpers_merge() {
    // The quantifier retry family: two self-looping retries (self-normalized
    // keys) plus a copy that jumps into one loop (raw key).
    let mut nfa = graph(
        vec![
            eps(0, vec![10, 20, 30]),
            eps(1, vec![]),
            nav_next(10, vec![1, 10]), // retry A, self-loop
            nav_next(20, vec![1, 20]), // retry B, self-loop twin of A
            nav_next(30, vec![1, 20]), // jumper into B's loop
        ],
        0,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 1, 10]);
    assert_eq!(match_at(&nfa, 0).successors, vec![Label(10)]);
    assert_eq!(match_at(&nfa, 10).successors, vec![Label(1), Label(10)]);
}

#[test]
fn different_effects_do_not_merge() {
    let mut nfa = graph(
        vec![
            eps(0, vec![10, 20]),
            MatchIR::epsilon(Label(10), Label(1))
                .prepend_effect(EffectIR::start_struct())
                .into(),
            MatchIR::epsilon(Label(20), Label(1))
                .prepend_effect(EffectIR::end_struct())
                .into(),
            eps(1, vec![]),
        ],
        0,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 10, 20, 1]);
}

#[test]
fn call_references_rewritten() {
    // A call whose continuation and target point at a merged twin follows the
    // representative afterwards.
    let mut nfa = graph(
        vec![
            eps(5, vec![1]),
            eps(6, vec![1]), // twin of 5
            eps(1, vec![]),
            CallIR::new(Label(2), ReturnAddr(Label(6)), CalleeEntry(Label(6))).into(),
        ],
        6,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![5, 1, 2]);
    let call = nfa
        .instructions
        .iter()
        .find_map(|i| match i {
            InstructionIR::Call(c) => Some(c),
            _ => None,
        })
        .expect("call present");
    assert_eq!(call.next, Label(5));
    assert_eq!(call.target, Label(5));
    assert_eq!(nfa.def_entries[&DefId::from_raw(0)], Label(5));
    assert_eq!(nfa.preamble_entry, Label(5));
}

#[test]
fn identical_calls_merge() {
    let mut nfa = graph(
        vec![
            eps(0, vec![2, 3]),
            eps(1, vec![]),
            CallIR::new(Label(2), ReturnAddr(Label(1)), CalleeEntry(Label(1))).into(),
            CallIR::new(Label(3), ReturnAddr(Label(1)), CalleeEntry(Label(1))).into(),
        ],
        0,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 1, 2]);
    assert_eq!(match_at(&nfa, 0).successors, vec![Label(2)]);
}

#[test]
fn returns_never_merge() {
    let mut nfa = graph(
        vec![
            eps(0, vec![1, 2]),
            ReturnIR::new(Label(1)).into(),
            ReturnIR::new(Label(2)).into(),
        ],
        0,
    );

    dedup_states(&mut nfa);

    assert_eq!(labels(&nfa), vec![0, 1, 2]);
    assert_eq!(match_at(&nfa, 0).successors, vec![Label(1), Label(2)]);
}
