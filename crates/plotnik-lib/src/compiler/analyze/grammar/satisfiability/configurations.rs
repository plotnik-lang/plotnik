//! Reachable automaton positions and their path-specific sibling boundaries.

use super::AnalysisLimit;
use super::automaton::State;
use super::budget::Budget;
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

/// Only the occupied word interval is stored. A set containing a late exit does
/// not allocate all the zero words between it and the automaton's start.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Configurations {
    first_word: usize,
    words: Vec<u64>,
}

impl Configurations {
    pub(super) fn singleton(
        state: Configuration,
        budget: &mut Budget,
    ) -> Result<Self, AnalysisLimit> {
        let mut set = Self::default();
        set.insert(state, budget)?;
        Ok(set)
    }

    pub(super) fn word_count(&self) -> u64 {
        self.words.len() as u64
    }

    fn insertion_work(&self, state: Configuration) -> u64 {
        let (word, _) = Self::position(state);
        self.extension_work(word, word + 1) + 1
    }

    fn union_work(&self, other: &Self) -> u64 {
        if other.is_empty() {
            return 1;
        }

        self.extension_work(other.first_word, other.end_word()) + other.word_count()
    }

    fn comparison_work(&self, other: &Self) -> u64 {
        if self.first_word != other.first_word || self.words.len() != other.words.len() {
            return 1;
        }

        self.word_count() + 1
    }

    pub(super) fn copy(&self, budget: &mut Budget) -> Result<Self, AnalysisLimit> {
        budget.spend(self.word_count() + 1)?;
        Ok(self.clone())
    }

    pub(super) fn equals(&self, other: &Self, budget: &mut Budget) -> Result<bool, AnalysisLimit> {
        budget.spend(self.comparison_work(other))?;
        Ok(self == other)
    }

    /// Insert `state`, returning whether it was newly added.
    pub(super) fn insert(
        &mut self,
        state: Configuration,
        budget: &mut Budget,
    ) -> Result<bool, AnalysisLimit> {
        budget.spend(self.insertion_work(state))?;
        let (word, bit) = Self::position(state);
        self.extend_to(word, word + 1);
        let offset = word - self.first_word;
        let mask = 1u64 << bit;
        let absent = self.words[offset] & mask == 0;
        self.words[offset] |= mask;
        Ok(absent)
    }

    pub(super) fn contains_state(&self, state: State) -> bool {
        BoundaryState::all().any(|boundary| {
            let (word, bit) = Self::position(Configuration::new(state, boundary));
            self.word(word).is_some_and(|word| word & (1 << bit) != 0)
        })
    }

    /// Union `other` into `self`, returning whether `self` grew.
    pub(super) fn union_with(
        &mut self,
        other: &Configurations,
        budget: &mut Budget,
    ) -> Result<bool, AnalysisLimit> {
        budget.spend(self.union_work(other))?;
        if other.is_empty() {
            return Ok(false);
        }

        self.extend_to(other.first_word, other.end_word());
        let offset = other.first_word - self.first_word;
        let mut changed = false;
        for (word, &incoming) in other.words.iter().enumerate() {
            let target = &mut self.words[offset + word];
            let merged = *target | incoming;
            changed |= merged != *target;
            *target = merged;
        }
        Ok(changed)
    }

    pub(super) fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub(super) fn iter<'a>(
        &'a self,
        budget: &mut Budget,
    ) -> Result<impl Iterator<Item = Configuration> + use<'a>, AnalysisLimit> {
        budget.spend(self.word_count())?;
        let bits: u64 = self.words.iter().map(|word| word.count_ones() as u64).sum();
        budget.spend(self.word_count() + bits)?;
        // Scan the occupied word interval, then walk each word's set bits. Late
        // exit states do not force a scan of every preceding zero word.
        Ok(self.words.iter().enumerate().flat_map(|(word, &bits)| {
            let base = (self.first_word + word) as u32 * u64::BITS;
            let mut rest = bits;
            std::iter::from_fn(move || {
                if rest == 0 {
                    return None;
                }
                let bit = rest.trailing_zeros();
                rest &= rest - 1;
                Some(Configuration::from_index(base + bit))
            })
        }))
    }

    fn position(state: Configuration) -> (usize, u32) {
        let index = state.index();
        ((index / u64::BITS) as usize, index % u64::BITS)
    }

    fn word(&self, index: usize) -> Option<u64> {
        let offset = index.checked_sub(self.first_word)?;
        self.words.get(offset).copied()
    }

    fn end_word(&self) -> usize {
        self.first_word + self.words.len()
    }

    fn extension_work(&self, first: usize, end: usize) -> u64 {
        if self.is_empty() {
            return (end - first) as u64;
        }

        let new_first = self.first_word.min(first);
        let new_end = self.end_word().max(end);
        let new_len = new_end - new_first;
        if new_len == self.words.len() {
            return 0;
        }

        let initialized = new_len - self.words.len();
        let reallocated = if new_len > self.words.capacity() {
            self.words.len()
        } else {
            0
        };
        let prefix = self.first_word - new_first;
        let shifted = if prefix != 0 {
            self.words.len() + prefix
        } else {
            0
        };
        (initialized + reallocated + shifted) as u64
    }

    /// Insertion and union are the only mutations. Each adds a nonzero boundary
    /// word, so there are no empty prefixes or suffixes and equality is structural.
    fn extend_to(&mut self, first: usize, end: usize) {
        if self.is_empty() {
            self.first_word = first;
            self.words.resize(end - first, 0);
            return;
        }

        let new_first = self.first_word.min(first);
        let new_end = self.end_word().max(end);
        let prefix = self.first_word - new_first;
        let previous_len = self.words.len();
        self.words.resize(new_end - new_first, 0);
        if prefix != 0 {
            self.words.copy_within(0..previous_len, prefix);
            self.words[..prefix].fill(0);
        }
        self.first_word = new_first;
    }
}
