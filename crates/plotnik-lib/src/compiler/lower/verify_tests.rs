use crate::bytecode::Nav;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{
    DefSpecialization, EffectIR, InstructionIR, Label, MatchIR, NfaGraph, ReturnIR,
};
use indexmap::IndexMap;

#[test]
#[should_panic(expected = "cursor-depth imbalance")]
fn unbalanced_body_depth_panics() {
    let nfa = NfaGraph {
        instructions: vec![
            InstructionIR::from(MatchIR::epsilon(Label(0), Label(1)).nav(Nav::Down)),
            InstructionIR::from(ReturnIR::new(Label(1))),
        ],
        def_entries: {
            let mut entries = IndexMap::new();
            entries.insert(DefSpecialization::ordinary(DefId::from_raw(0)), Label(0));
            entries
        },
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    super::debug_impl::assert_depth_neutrality(&nfa, "test");
}

#[test]
#[should_panic(expected = "empty-match Node effect")]
fn empty_match_node_effect_panics() {
    let nfa = NfaGraph {
        instructions: vec![
            InstructionIR::from(
                MatchIR::epsilon(Label(0), Label(1)).append_effect(EffectIR::node()),
            ),
            InstructionIR::from(ReturnIR::new(Label(1))),
        ],
        def_entries: {
            let mut entries = IndexMap::new();
            entries.insert(DefSpecialization::ordinary(DefId::from_raw(0)), Label(0));
            entries
        },
        entry_point_wrappers: Default::default(),
        spans: None,
        label_origins: Vec::new(),
    };

    super::debug_impl::assert_no_node_on_empty_paths(&nfa, "test");
}
