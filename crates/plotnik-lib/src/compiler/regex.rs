//! Compiler-owned regex representations for generated executors.
//!
//! Regex syntax and matching semantics come from one compiler-owned build
//! configuration: every representation uses the same minimized, unanchored
//! dense DFA. Rust and bytecode serialize its native sparse form; dynamic
//! targets render the portable table below and ship a tiny byte walker.

use std::collections::{BTreeMap, VecDeque};

use regex_automata::dfa::{Automaton, dense};
use regex_automata::util::primitives::StateID;
use regex_automata::util::start;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortableDfa {
    pub(crate) start: PortableStateId,
    pub(crate) states: Vec<PortableState>,
}

impl PortableDfa {
    pub(crate) fn validate(&self) {
        let state_count = self.states.len();
        assert!(state_count > 0, "portable DFA has a start state");
        assert!(
            self.start.index() < state_count,
            "DFA start state is in bounds"
        );
        for state in &self.states {
            assert!(
                state.default.index() < state_count,
                "DFA default transition is in bounds"
            );
            assert!(
                state.eoi.index() < state_count,
                "DFA EOI transition is in bounds"
            );
            assert!(
                !(state.accepting && (state.dead || state.quit)) && !(state.dead && state.quit),
                "DFA special-state classes are disjoint"
            );
            let mut previous_end = None;
            for transition in &state.transitions {
                assert!(
                    transition.start <= transition.end,
                    "DFA byte range is non-empty"
                );
                assert!(
                    previous_end.is_none_or(|end| end < transition.start),
                    "DFA byte ranges are ordered and disjoint"
                );
                assert!(
                    transition.next.index() < state_count,
                    "DFA byte transition is in bounds"
                );
                assert_ne!(
                    transition.next, state.default,
                    "DFA ranges omit transitions to the default state"
                );
                previous_end = Some(transition.end);
            }
        }
    }

    #[cfg(test)]
    pub(super) fn is_match(&self, bytes: &[u8]) -> Result<bool, ()> {
        let mut state = self.start;
        for &byte in bytes {
            state = self.states[state.index()].next(byte);
            let current = &self.states[state.index()];
            if current.accepting {
                return Ok(true);
            }
            if current.dead {
                return Ok(false);
            }
            if current.quit {
                return Err(());
            }
        }

        state = self.states[state.index()].eoi;
        // regex-automata's forward search only tests the EOI state for a
        // match; quit handling applies to real input-byte transitions.
        Ok(self.states[state.index()].accepting)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortableStateId(pub(crate) u32);

impl PortableStateId {
    fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).expect("regex DFA state count fits u32"))
    }

    fn index(self) -> usize {
        usize::try_from(self.0).expect("portable DFA state id originated as usize")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PortableState {
    pub(crate) accepting: bool,
    pub(crate) dead: bool,
    pub(crate) quit: bool,
    pub(crate) eoi: PortableStateId,
    pub(crate) default: PortableStateId,
    pub(crate) transitions: Vec<PortableTransition>,
}

impl PortableState {
    #[cfg(test)]
    fn next(&self, byte: u8) -> PortableStateId {
        self.transitions
            .iter()
            .find(|transition| transition.start <= byte && byte <= transition.end)
            .map_or(self.default, |transition| transition.next)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortableTransition {
    pub(crate) start: u8,
    pub(crate) end: u8,
    pub(crate) next: PortableStateId,
}

/// Compile native sparse-DFA bytes for the Rust runtime and bytecode module.
pub(in crate::compiler) fn compile_native_dfa(pattern: &str) -> Result<Vec<u8>, String> {
    let dense = build_dense(pattern)?;
    let sparse = dense.to_sparse().map_err(|error| error.to_string())?;
    Ok(sparse.to_bytes_little_endian())
}

/// Compile a target-independent sparse byte-DFA table.
pub(crate) fn compile_portable_dfa(pattern: &str) -> Result<PortableDfa, String> {
    let dfa = build_dense(pattern)?;
    let start = dfa
        .start_state(&start::Config::new())
        .map_err(|error| error.to_string())?;

    let mut ids = BTreeMap::new();
    let mut queue = VecDeque::new();
    intern_state(start, &mut ids, &mut queue);
    let mut raw_states = Vec::new();
    while let Some(state) = queue.pop_front() {
        let mut transitions = Vec::with_capacity(256);
        for byte in u8::MIN..=u8::MAX {
            let next = dfa.next_state(state, byte);
            intern_state(next, &mut ids, &mut queue);
            transitions.push(next);
        }
        let eoi = dfa.next_eoi_state(state);
        intern_state(eoi, &mut ids, &mut queue);
        raw_states.push(RawState {
            accepting: dfa.is_match_state(state),
            dead: dfa.is_dead_state(state),
            quit: dfa.is_quit_state(state),
            eoi,
            transitions,
        });
    }

    let states = raw_states
        .into_iter()
        .map(|state| portable_state(state, &ids))
        .collect();
    let portable = PortableDfa {
        start: PortableStateId::from_index(0),
        states,
    };
    portable.validate();
    Ok(portable)
}

fn build_dense(pattern: &str) -> Result<dense::DFA<Vec<u32>>, String> {
    dense::DFA::builder()
        .configure(
            dense::DFA::config()
                .start_kind(regex_automata::dfa::StartKind::Unanchored)
                .minimize(true),
        )
        .build(pattern)
        .map_err(|error| error.to_string())
}

struct RawState {
    accepting: bool,
    dead: bool,
    quit: bool,
    eoi: StateID,
    transitions: Vec<StateID>,
}

fn intern_state(
    state: StateID,
    ids: &mut BTreeMap<StateID, PortableStateId>,
    queue: &mut VecDeque<StateID>,
) -> PortableStateId {
    if let Some(&id) = ids.get(&state) {
        return id;
    }
    let id = PortableStateId::from_index(ids.len());
    ids.insert(state, id);
    queue.push_back(state);
    id
}

fn portable_state(state: RawState, ids: &BTreeMap<StateID, PortableStateId>) -> PortableState {
    let default = default_transition(&state.transitions);
    let mut transitions = Vec::new();
    let mut byte = 0;
    while byte < state.transitions.len() {
        let next = state.transitions[byte];
        if next == default {
            byte += 1;
            continue;
        }

        let start = byte;
        while byte + 1 < state.transitions.len() && state.transitions[byte + 1] == next {
            byte += 1;
        }
        transitions.push(PortableTransition {
            start: u8::try_from(start).expect("byte transition starts within u8"),
            end: u8::try_from(byte).expect("byte transition ends within u8"),
            next: resolve_state(ids, next),
        });
        byte += 1;
    }

    PortableState {
        accepting: state.accepting,
        dead: state.dead,
        quit: state.quit,
        eoi: resolve_state(ids, state.eoi),
        default: resolve_state(ids, default),
        transitions,
    }
}

fn default_transition(transitions: &[StateID]) -> StateID {
    let mut frequencies = BTreeMap::<StateID, (usize, usize)>::new();
    for (index, &state) in transitions.iter().enumerate() {
        let frequency = frequencies.entry(state).or_insert((0, index));
        frequency.0 += 1;
    }
    frequencies
        .into_iter()
        .max_by(|(_, left), (_, right)| left.0.cmp(&right.0).then_with(|| right.1.cmp(&left.1)))
        .map(|(state, _)| state)
        .expect("every DFA state has 256 byte transitions")
}

fn resolve_state(ids: &BTreeMap<StateID, PortableStateId>, state: StateID) -> PortableStateId {
    *ids.get(&state)
        .expect("BFS records every reachable DFA transition")
}
