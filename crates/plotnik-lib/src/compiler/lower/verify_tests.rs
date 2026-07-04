use crate::bytecode::Nav;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{EffectIR, InstructionIR, Label, MatchIR, NfaGraph, ReturnIR};
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
            entries.insert(DefId::from_raw(0), Label(0));
            entries
        },
        def_entries_consuming: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    super::debug_impl::assert_depth_neutrality(&nfa, "test");
}

#[test]
#[should_panic(expected = "zero-width Node effect")]
fn zero_width_node_effect_panics() {
    let nfa = NfaGraph {
        instructions: vec![
            InstructionIR::from(
                MatchIR::epsilon(Label(0), Label(1)).append_effect(EffectIR::node()),
            ),
            InstructionIR::from(ReturnIR::new(Label(1))),
        ],
        def_entries: {
            let mut entries = IndexMap::new();
            entries.insert(DefId::from_raw(0), Label(0));
            entries
        },
        def_entries_consuming: Default::default(),
        entrypoint_wrappers: Default::default(),
    };

    super::debug_impl::assert_no_node_on_zero_width_paths(&nfa, "test");
}
