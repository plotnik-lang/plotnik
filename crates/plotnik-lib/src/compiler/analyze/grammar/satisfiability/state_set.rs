//! A compact set of automaton states.
//!
//! `THREAD` values are state sets, churned through union and equality as the fixed
//! point converges, so the representation is a bit vector — one bit per state, dense
//! and small (an automaton has a handful of states). It auto-grows on insert, and
//! equality ignores trailing zero words so two sets compare equal whatever their
//! backing length.

use super::automaton::State;

#[derive(Clone, Debug, Default)]
pub(super) struct StateSet {
    words: Vec<u64>,
}

impl StateSet {
    pub(super) fn singleton(state: State) -> Self {
        let mut set = Self::default();
        set.insert(state);
        set
    }

    /// Insert `state`, returning whether it was newly added.
    pub(super) fn insert(&mut self, state: State) -> bool {
        let (word, bit) = Self::position(state);
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let mask = 1u64 << bit;
        let absent = self.words[word] & mask == 0;
        self.words[word] |= mask;
        absent
    }

    pub(super) fn contains(&self, state: State) -> bool {
        let (word, bit) = Self::position(state);
        self.words.get(word).is_some_and(|w| w & (1u64 << bit) != 0)
    }

    /// Union `other` into `self`, returning whether `self` grew.
    pub(super) fn union_with(&mut self, other: &StateSet) -> bool {
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

    pub(super) fn iter(&self) -> impl Iterator<Item = State> + '_ {
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
                Some(base + bit)
            })
        })
    }

    fn position(state: State) -> (usize, u32) {
        ((state / u64::BITS) as usize, state % u64::BITS)
    }
}

impl PartialEq for StateSet {
    fn eq(&self, other: &Self) -> bool {
        let len = self.words.len().max(other.words.len());
        (0..len).all(|i| self.word(i) == other.word(i))
    }
}

impl Eq for StateSet {}

impl StateSet {
    fn word(&self, index: usize) -> u64 {
        self.words.get(index).copied().unwrap_or(0)
    }
}
