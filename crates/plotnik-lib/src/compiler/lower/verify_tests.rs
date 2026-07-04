use crate::bytecode::Nav;
use crate::compiler::ids::DefId;
use crate::compiler::lower::ir::{InstructionIR, Label, MatchIR, NfaGraph, ReturnIR};
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
        entrypoint_wrappers: Default::default(),
    };

    super::debug_impl::assert_depth_neutrality(&nfa, "test");
}
