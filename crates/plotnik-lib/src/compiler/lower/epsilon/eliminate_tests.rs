use super::*;
use crate::bytecode::Nav;
use crate::compiler::lower::ir::{CallIR, CalleeEntry, EffectIR, ReturnAddr, ReturnIR};

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

fn make_epsilon_with_start(label: u32, succs: Vec<u32>) -> InstructionIR {
    InstructionIR::Match(
        MatchIR::terminal(Label(label))
            .nav(Nav::Epsilon)
            .append_effect(EffectIR::record_open())
            .successors(succs.into_iter().map(Label).collect()),
    )
}

fn make_epsilon_with_end(label: u32, succs: Vec<u32>) -> InstructionIR {
    InstructionIR::Match(
        MatchIR::terminal(Label(label))
            .nav(Nav::Epsilon)
            .append_effect(EffectIR::record_close())
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

    let (target, effects) = InstrIndex::new(&instructions, &idx)
        .see_through(Label(0))
        .unwrap();
    assert_eq!(target, Label(2));
    assert!(effects.is_empty());
}

#[test]
fn see_through_with_effects() {
    // 0 (ε+RecordOpen) → 1 (ε+RecordClose) → 2 (match)
    let instructions = vec![
        make_epsilon_with_start(0, vec![1]),
        make_epsilon_with_end(1, vec![2]),
        make_match(2, Nav::Down, vec![]),
    ];
    let idx = build_label_to_index(&instructions);

    let (target, effects) = InstrIndex::new(&instructions, &idx)
        .see_through(Label(0))
        .unwrap();
    assert_eq!(target, Label(2));
    assert_eq!(effects.len(), 2); // RecordOpen from 0, RecordClose from 1
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
    let (target, effects) = InstrIndex::new(&instructions, &idx)
        .see_through(Label(0))
        .unwrap();
    assert_eq!(target, Label(1)); // Stops at branching epsilon
    assert!(effects.is_empty());

    // Starting from branching epsilon returns itself
    let (target, effects) = InstrIndex::new(&instructions, &idx)
        .see_through(Label(1))
        .unwrap();
    assert_eq!(target, Label(1));
    assert!(effects.is_empty());
}

#[test]
fn forward_migrate_to_exclusive_successor() {
    // 0 (ε+RecordOpen) → 1 (match), only 0 points to 1
    let mut instructions = vec![
        make_epsilon_with_start(0, vec![1]),
        make_match(1, Nav::Down, vec![]),
    ];

    forward_migrate(&mut instructions);

    let eps = match &instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert!(eps.effects.is_empty());

    let m1 = match &instructions[1] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m1.effects.len(), 1);
}

#[test]
fn forward_migrate_blocked_by_multi_pred() {
    // 0, 2 both point to 1 (match)
    // ε can't forward-migrate because 1 has multiple preds
    let mut instructions = vec![
        make_epsilon_with_start(0, vec![1]),
        make_match(1, Nav::Down, vec![]),
        make_match(2, Nav::Down, vec![1]),
    ];

    forward_migrate(&mut instructions);

    // Effects NOT moved (1 has multiple predecessors)
    let eps = match &instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(eps.effects.len(), 1); // Still has effect
}

#[test]
fn laser_vision_single_succ_absorbs_effects() {
    // 0 (match, single succ) → 1 (ε+RecordOpen) → 2 (match)
    let instructions = vec![
        make_match(0, Nav::Down, vec![1]),
        make_epsilon_with_start(1, vec![2]),
        make_match(2, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    laser_vision(&mut result);

    // 0 absorbed effects and now points to 2
    let m0 = match &result.instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m0.successors, vec![Label(2)]);
    assert_eq!(m0.effects.len(), 1);
}

#[test]
fn laser_vision_multi_succ_effectless_only() {
    // 0 (match) → [1 (ε), 3]
    // 1 (ε+RecordOpen) → 2
    let instructions = vec![
        make_match(0, Nav::Down, vec![1, 3]),
        make_epsilon_with_start(1, vec![2]),
        make_match(2, Nav::Next, vec![]),
        make_match(3, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    laser_vision(&mut result);

    // 0 can't absorb effects (multi-succ), so 1 stays
    let m0 = match &result.instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m0.successors, vec![Label(1), Label(3)]);
    assert!(m0.effects.is_empty());
}

#[test]
fn laser_vision_epsilon_source_absorbs_chain() {
    // 0 (ε+RecordOpen) → 1 (ε+RecordClose) → 2 (match)
    // The head epsilon absorbs the chain: 0 (ε, RecordOpen, RecordClose) → 2
    let instructions = vec![
        make_epsilon_with_start(0, vec![1]),
        make_epsilon_with_end(1, vec![2]),
        make_match(2, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    laser_vision(&mut result);

    let m0 = match &result.instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m0.successors, vec![Label(2)]);
    assert_eq!(m0.effects.len(), 2);
}

#[test]
fn laser_vision_branching_epsilon_skips_pure_jump() {
    // 0 (ε+RecordOpen, branching) → [1 (ε pure) → 3, 4]
    // The pure jump is bypassed per-successor; effects stay on the branch point.
    let instructions = vec![
        make_epsilon_with_start(0, vec![1, 4]),
        make_epsilon(1, vec![3]),
        make_match(3, Nav::Next, vec![]),
        make_match(4, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    laser_vision(&mut result);

    let m0 = match &result.instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m0.successors, vec![Label(3), Label(4)]);
    assert_eq!(m0.effects.len(), 1);
}

#[test]
fn epsilon_chain_around_call_coalesces() {
    // Scope brackets around a Call ride separate epsilons (CallIR carries no
    // effects). The chain head must absorb the rest:
    // 0 (ε+RecordOpen) → 1 (ε+RecordClose) → 2 (call → 4, next 3)
    let instructions = vec![
        make_epsilon_with_start(0, vec![1]),
        make_epsilon_with_end(1, vec![2]),
        CallIR::new(Label(2), ReturnAddr(Label(3)), CalleeEntry(Label(4))).into(),
        make_match(3, Nav::Next, vec![]),
        ReturnIR::new(Label(4)).into(),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    eliminate_epsilons(&mut result);

    let m0 = match &result.instructions[0] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m0.successors, vec![Label(2)]);
    assert_eq!(m0.effects.len(), 2);
}

#[test]
fn combined_forward_then_laser() {
    // The tricky case:
    // 0 (match) → [1 (ε+RecordOpen), 3]
    // 1 → 2 (match), only 1 points to 2
    //
    // Phase A: 1 forward-migrates RecordOpen to 2, 1 becomes effectless
    // Phase B: 0 sees through 1 (now effectless) to 2
    let instructions = vec![
        make_match(0, Nav::Down, vec![1, 3]),
        make_epsilon_with_start(1, vec![2]),
        make_match(2, Nav::Next, vec![]),
        make_match(3, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    forward_migrate(&mut result.instructions);

    // 1 should now be effectless, 2 has the effect
    let eps = match &result.instructions[1] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert!(eps.effects.is_empty());

    let m2 = match &result.instructions[2] {
        InstructionIR::Match(m) => m,
        _ => panic!(),
    };
    assert_eq!(m2.effects.len(), 1);

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

    let mut result = NfaGraph {
        instructions,
        def_entries: {
            let mut m = indexmap::IndexMap::new();
            m.insert(
                crate::compiler::lower::ir::DefVariant::ordinary(
                    crate::compiler::ids::DefId::from_raw(0),
                ),
                Label(0),
            );
            m
        },
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    laser_vision(&mut result);

    assert_eq!(
        result.def_entries[&crate::compiler::lower::ir::DefVariant::ordinary(
            crate::compiler::ids::DefId::from_raw(0),
        )],
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

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
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

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
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

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
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

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
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
        make_epsilon_with_start(1, vec![2, 3]),
        make_match(2, Nav::Next, vec![]),
        make_match(3, Nav::Next, vec![]),
    ];

    let mut result = NfaGraph {
        instructions,
        def_entries: indexmap::IndexMap::new(),
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
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
