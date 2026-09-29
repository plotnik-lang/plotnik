//! Reachable automaton positions and their path-specific sibling boundaries.

use super::automaton::State;
use crate::compiler::analyze::boundary::{BoundaryState, FirstClass};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct Configuration {
    pub(super) state: State,
    pub(super) boundary: BoundaryState,
}

impl Configuration {
    pub(super) fn new(state: State, boundary: BoundaryState) -> Self {
        Self { state, boundary }
    }

    pub(super) fn start(state: State) -> Self {
        Self::new(state, BoundaryState::START)
    }

    pub(super) fn consume(self, state: State, class: FirstClass) -> Self {
        Self::new(state, self.boundary.consume(class))
    }

    fn index(self) -> u32 {
        self.state * BoundaryState::COUNT as u32 + self.boundary.index() as u32
    }

    fn from_index(index: u32) -> Self {
        Self::new(
            index / BoundaryState::COUNT as u32,
            BoundaryState::from_index(index as usize % BoundaryState::COUNT),
        )
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct Configurations {
    words: Vec<u64>,
}

impl Configurations {
    pub(super) fn singleton(state: Configuration) -> Self {
        let mut set = Self::default();
        set.insert(state);
        set
    }

    /// Insert `state`, returning whether it was newly added.
    pub(super) fn insert(&mut self, state: Configuration) -> bool {
        let (word, bit) = Self::position(state);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let mask = 1u64 << bit;
        let absent = self.words[word] & mask == 0;
        self.words[word] |= mask;
        absent
    }

    pub(super) fn contains_state(&self, state: State) -> bool {
        BoundaryState::all().any(|boundary| {
            let (word, bit) = Self::position(Configuration::new(state, boundary));
            self.words
                .get(word)
                .is_some_and(|word| word & (1 << bit) != 0)
        })
    }

    /// Union `other` into `self`, returning whether `self` grew.
    pub(super) fn union_with(&mut self, other: &Configurations) -> bool {
        if other.words.len() > self.words.len() {
            self.words.resize(other.words.len(), 0);
        }
        let mut changed = false;
        for (word, &incoming) in other.words.iter().enumerate() {
            let merged = self.words[word] | incoming;
            changed |= merged != self.words[word];
            self.words[word] = merged;
        }
        changed
    }

    pub(super) fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = Configuration> + '_ {
        // Walk set bits directly via `trailing_zeros` (O(popcount)) rather than testing
        // all 64 bits of every word — `iter` is the satisfiability solver's hot frame.
        self.words.iter().enumerate().flat_map(|(word, &bits)| {
            let base = word as u32 * u64::BITS;
            let mut rest = bits;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros();
                rest &= rest - 1;
                Some(Configuration::from_index(base + bit))
            })
        })
    }

    fn position(state: Configuration) -> (usize, u32) {
        let index = state.index();
        ((index / u64::BITS) as usize, index % u64::BITS)
    }
}

impl PartialEq for Configurations {
    fn eq(&self, other: &Self) -> bool {
        let len = self.words.len().max(other.words.len());
        (0..len).all(|i| self.word(i) == other.word(i))
    }
}

impl Eq for Configurations {}

impl Configurations {
    fn word(&self, index: usize) -> u64 {
        self.words.get(index).copied().unwrap_or(0)
    }
}
