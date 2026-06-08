use std::{
    char,
    cmp::{Ordering, max},
    fmt,
    iter::ExactSizeIterator,
    mem::{self, swap},
    ops::{Range, RangeInclusive},
};

/// A set of characters represented as a vector of ranges.
#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct CharacterSet {
    ranges: Vec<Range<u32>>,
}

/// A state in an NFA representing a regular grammar.
#[derive(Debug, PartialEq, Eq)]
pub enum NfaState {
    Advance {
        chars: CharacterSet,
        state_id: u32,
        is_sep: bool,
        precedence: i32,
    },
    Split(u32, u32),
    Accept {
        variable_index: usize,
        precedence: i32,
    },
}

#[derive(PartialEq, Eq, Default)]
pub struct Nfa {
    pub states: Vec<NfaState>,
}

#[derive(Debug)]
pub struct NfaCursor<'a> {
    pub(crate) state_ids: Vec<u32>,
    nfa: &'a Nfa,
}

#[derive(Debug, PartialEq, Eq)]
pub struct NfaTransition {
    pub characters: CharacterSet,
    pub is_separator: bool,
    pub precedence: i32,
    pub states: Vec<u32>,
}

const END: u32 = char::MAX as u32 + 1;

impl CharacterSet {
    /// Create a character set with a single character.
    #[must_use]
    pub const fn empty() -> Self {
        Self { ranges: Vec::new() }
    }

    /// Create a character set with a single character.
    #[must_use]
    #[expect(
        clippy::single_range_in_vec_init,
        reason = "Vec is the backing store for CharacterSet"
    )]
    pub fn from_char(c: char) -> Self {
        Self {
            ranges: vec![(c as u32)..(c as u32 + 1)],
        }
    }

    /// Create a character set containing all characters *not* present
    /// in this character set.
    #[must_use]
    pub fn negate(mut self) -> Self {
        let mut i = 0;
        let mut previous_end = 0;
        while i < self.ranges.len() {
            let range = &mut self.ranges[i];
            let start = previous_end;
            previous_end = range.end;
            if start < range.start {
                self.ranges[i] = start..range.start;
                i += 1;
            } else {
                self.ranges.remove(i);
            }
        }
        if previous_end < END {
            self.ranges.push(previous_end..END);
        }
        self
    }

    #[must_use]
    pub fn add_char(mut self, c: char) -> Self {
        self.add_int_range(0, c as u32, c as u32 + 1);
        self
    }

    #[must_use]
    pub fn add_range(mut self, start: char, end: char) -> Self {
        self.add_int_range(0, start as u32, end as u32 + 1);
        self
    }

    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, other: &Self) -> Self {
        let mut index = 0;
        for range in &other.ranges {
            index = self.add_int_range(index, range.start, range.end);
        }
        self
    }

    pub fn assign(&mut self, other: &Self) {
        self.ranges.clear();
        self.ranges.extend_from_slice(&other.ranges);
    }

    fn add_int_range(&mut self, mut i: usize, start: u32, end: u32) -> usize {
        while i < self.ranges.len() {
            let range = &mut self.ranges[i];
            if range.start > end {
                self.ranges.insert(i, start..end);
                return i;
            }
            if range.end >= start {
                range.end = range.end.max(end);
                range.start = range.start.min(start);

                // Join this range with the next range if needed.
                while i + 1 < self.ranges.len() && self.ranges[i + 1].start <= self.ranges[i].end {
                    self.ranges[i].end = self.ranges[i].end.max(self.ranges[i + 1].end);
                    self.ranges.remove(i + 1);
                }

                return i;
            }
            i += 1;
        }
        self.ranges.push(start..end);
        i
    }

    #[must_use]
    pub fn does_intersect(&self, other: &Self) -> bool {
        let mut left_ranges = self.ranges.iter();
        let mut right_ranges = other.ranges.iter();
        let mut left_range = left_ranges.next();
        let mut right_range = right_ranges.next();
        while let (Some(left), Some(right)) = (&left_range, &right_range) {
            if left.end <= right.start {
                left_range = left_ranges.next();
            } else if left.start >= right.end {
                right_range = right_ranges.next();
            } else {
                return true;
            }
        }
        false
    }

    #[must_use]
    pub fn is_subset_of(&self, other: &Self) -> bool {
        let mut other_ranges = other.ranges.iter();
        let mut other_range = other_ranges.next();

        'left: for left in &self.ranges {
            while let Some(right) = other_range {
                if right.end <= left.start {
                    other_range = other_ranges.next();
                    continue;
                }
                if right.start <= left.start && left.end <= right.end {
                    continue 'left;
                }
                return false;
            }
            return false;
        }

        true
    }

    /// Get the set of characters that are present in both this set
    /// and the other set. Remove those common characters from both
    /// of the operands.
    #[allow(clippy::return_self_not_must_use)]
    pub fn remove_intersection(&mut self, other: &mut Self) -> Self {
        let mut intersection = Vec::new();
        let mut left_i = 0;
        let mut right_i = 0;
        while left_i < self.ranges.len() && right_i < other.ranges.len() {
            let left = &mut self.ranges[left_i];
            let right = &mut other.ranges[right_i];

            match left.start.cmp(&right.start) {
                Ordering::Less => {
                    // [ L ]
                    //     [ R ]
                    if left.end <= right.start {
                        left_i += 1;
                        continue;
                    }

                    match left.end.cmp(&right.end) {
                        // [ L ]
                        //   [ R ]
                        Ordering::Less => {
                            intersection.push(right.start..left.end);
                            swap(&mut left.end, &mut right.start);
                            left_i += 1;
                        }

                        // [  L  ]
                        //   [ R ]
                        Ordering::Equal => {
                            intersection.push(right.clone());
                            left.end = right.start;
                            other.ranges.remove(right_i);
                        }

                        // [   L   ]
                        //   [ R ]
                        Ordering::Greater => {
                            intersection.push(right.clone());
                            let new_range = left.start..right.start;
                            left.start = right.end;
                            self.ranges.insert(left_i, new_range);
                            other.ranges.remove(right_i);
                            left_i += 1;
                        }
                    }
                }
                // [ L ]
                // [  R  ]
                Ordering::Equal if left.end < right.end => {
                    intersection.push(left.start..left.end);
                    right.start = left.end;
                    self.ranges.remove(left_i);
                }
                // [ L ]
                // [ R ]
                Ordering::Equal if left.end == right.end => {
                    intersection.push(left.clone());
                    self.ranges.remove(left_i);
                    other.ranges.remove(right_i);
                }
                // [  L  ]
                // [ R ]
                Ordering::Equal if left.end > right.end => {
                    intersection.push(right.clone());
                    left.start = right.end;
                    other.ranges.remove(right_i);
                }
                Ordering::Equal => {}
                Ordering::Greater => {
                    //     [ L ]
                    // [ R ]
                    if left.start >= right.end {
                        right_i += 1;
                        continue;
                    }

                    match left.end.cmp(&right.end) {
                        //   [ L ]
                        // [   R   ]
                        Ordering::Less => {
                            intersection.push(left.clone());
                            let new_range = right.start..left.start;
                            right.start = left.end;
                            other.ranges.insert(right_i, new_range);
                            self.ranges.remove(left_i);
                            right_i += 1;
                        }

                        //   [ L ]
                        // [  R  ]
                        Ordering::Equal => {
                            intersection.push(left.clone());
                            right.end = left.start;
                            self.ranges.remove(left_i);
                        }

                        //   [   L   ]
                        // [   R   ]
                        Ordering::Greater => {
                            intersection.push(left.start..right.end);
                            swap(&mut left.start, &mut right.end);
                            right_i += 1;
                        }
                    }
                }
            }
        }
        Self {
            ranges: intersection,
        }
    }

    /// Produces a `CharacterSet` containing every character in `self` that is not present in
    /// `other`.
    #[allow(clippy::must_use_candidate, clippy::return_self_not_must_use)]
    pub fn difference(mut self, mut other: Self) -> Self {
        self.remove_intersection(&mut other);
        self
    }

    pub fn char_codes(&self) -> impl Iterator<Item = u32> + '_ {
        self.ranges.iter().flat_map(Clone::clone)
    }

    pub fn chars(&self) -> impl Iterator<Item = char> + '_ {
        self.char_codes().filter_map(char::from_u32)
    }

    #[must_use]
    pub const fn range_count(&self) -> usize {
        self.ranges.len()
    }

    pub fn ranges(&self) -> impl Iterator<Item = RangeInclusive<char>> + '_ {
        self.ranges.iter().filter_map(|range| {
            let start = range.clone().find_map(char::from_u32)?;
            let end = (range.start..range.end).rev().find_map(char::from_u32)?;
            Some(start..=end)
        })
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    #[must_use]
    pub fn contains_codepoint_range(&self, seek_range: Range<u32>) -> bool {
        let ix = match self.ranges.binary_search_by(|probe| {
            if probe.end <= seek_range.start {
                Ordering::Less
            } else if probe.start > seek_range.start {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        }) {
            Ok(ix) | Err(ix) => ix,
        };
        self.ranges
            .get(ix)
            .is_some_and(|range| range.start <= seek_range.start && range.end >= seek_range.end)
    }

    #[must_use]
    pub fn contains(&self, c: char) -> bool {
        self.contains_codepoint_range(c as u32..c as u32 + 1)
    }
}

impl Ord for CharacterSet {
    fn cmp(&self, other: &Self) -> Ordering {
        let count_cmp = self
            .ranges
            .iter()
            .map(ExactSizeIterator::len)
            .sum::<usize>()
            .cmp(&other.ranges.iter().map(ExactSizeIterator::len).sum());
        if count_cmp != Ordering::Equal {
            return count_cmp;
        }

        for (left_range, right_range) in self.ranges.iter().zip(other.ranges.iter()) {
            let cmp = left_range.len().cmp(&right_range.len());
            if cmp != Ordering::Equal {
                return cmp;
            }

            for (left, right) in left_range.clone().zip(right_range.clone()) {
                let cmp = left.cmp(&right);
                if cmp != Ordering::Equal {
                    return cmp;
                }
            }
        }
        Ordering::Equal
    }
}

impl PartialOrd for CharacterSet {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Debug for CharacterSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "CharacterSet [")?;
        let mut set = self.clone();
        if self.contains(char::MAX) {
            write!(f, "^ ")?;
            set = set.negate();
        }
        for (i, range) in set.ranges().enumerate() {
            if i > 0 {
                write!(f, ", ")?;
            }
            write!(f, "{range:?}")?;
        }
        write!(f, "]")?;
        Ok(())
    }
}

impl Nfa {
    #[must_use]
    pub const fn new() -> Self {
        Self { states: Vec::new() }
    }

    #[must_use]
    pub fn last_state_id(&self) -> u32 {
        assert!(!self.states.is_empty());
        self.states.len() as u32 - 1
    }
}

impl fmt::Debug for Nfa {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "Nfa {{ states: {{")?;
        for (i, state) in self.states.iter().enumerate() {
            writeln!(f, "  {i}: {state:?},")?;
        }
        write!(f, "}} }}")?;
        Ok(())
    }
}

impl<'a> NfaCursor<'a> {
    #[must_use]
    pub fn new(nfa: &'a Nfa, mut states: Vec<u32>) -> Self {
        let mut result = Self {
            nfa,
            state_ids: Vec::new(),
        };
        result.add_states(&mut states);
        result
    }

    pub fn reset(&mut self, mut states: Vec<u32>) {
        self.state_ids.clear();
        self.add_states(&mut states);
    }

    pub fn force_reset(&mut self, states: Vec<u32>) {
        self.state_ids = states;
    }

    pub fn transition_chars(&self) -> impl Iterator<Item = (&CharacterSet, bool)> {
        self.raw_transitions().map(|t| (t.0, t.1))
    }

    #[must_use]
    pub fn transitions(&self) -> Vec<NfaTransition> {
        Self::group_transitions(self.raw_transitions())
    }

    /// Like [`transitions()`](Self::transitions) but also returns whether any raw NFA transition
    /// is a separator. This is computed in the same pass, avoiding a second
    /// iteration over `state_ids` for callers that need both.
    #[must_use]
    pub fn transitions_and_any_sep(&self) -> (Vec<NfaTransition>, bool) {
        let mut any_sep = false;
        let result =
            Self::group_transitions(self.raw_transitions().map(|(chars, is_sep, prec, state)| {
                any_sep |= is_sep;
                (chars, is_sep, prec, state)
            }));
        (result, any_sep)
    }

    fn raw_transitions(&self) -> impl Iterator<Item = (&CharacterSet, bool, i32, u32)> {
        self.state_ids.iter().filter_map(move |id| {
            if let NfaState::Advance {
                chars,
                state_id,
                precedence,
                is_sep,
            } = &self.nfa.states[*id as usize]
            {
                Some((chars, *is_sep, *precedence, *state_id))
            } else {
                None
            }
        })
    }

    fn group_transitions<'b>(
        iter: impl Iterator<Item = (&'b CharacterSet, bool, i32, u32)>,
    ) -> Vec<NfaTransition> {
        let mut result = Vec::<NfaTransition>::new();
        // Reuse a single CharacterSet buffer across iterations to avoid one
        // malloc per raw transition. `assign` refills it in-place; `mem::take`
        // donates the allocation to a result entry when chars has a remainder.
        let mut chars = CharacterSet::empty();
        for (input_chars, is_sep, prec, state) in iter {
            chars.assign(input_chars);
            let mut i = 0;
            while i < result.len() && !chars.is_empty() {
                let intersection = result[i].characters.remove_intersection(&mut chars);
                if !intersection.is_empty() {
                    let chars_is_empty = result[i].characters.is_empty();
                    let mut intersection_states = if chars_is_empty {
                        mem::take(&mut result[i].states)
                    } else {
                        result[i].states.clone()
                    };
                    if let Err(j) = intersection_states.binary_search(&state) {
                        intersection_states.insert(j, state);
                    }
                    let intersection_transition = NfaTransition {
                        characters: intersection,
                        is_separator: result[i].is_separator && is_sep,
                        precedence: max(result[i].precedence, prec),
                        states: intersection_states,
                    };
                    if chars_is_empty {
                        result[i] = intersection_transition;
                    } else {
                        // Push to the tail instead of inserting at i (which
                        // would be O(n)).  After remove_intersection, the new
                        // `chars` (C') and `intersection` (I = A∩C) are
                        // disjoint, so when the loop later reaches I at the
                        // tail, remove_intersection(I, C'') will be a no-op.
                        // The final sort makes mid-loop ordering irrelevant.
                        result.push(intersection_transition);
                    }
                }
                i += 1;
            }
            if !chars.is_empty() {
                result.push(NfaTransition {
                    characters: mem::take(&mut chars),
                    precedence: prec,
                    states: vec![state],
                    is_separator: is_sep,
                });
            }
        }

        let mut i = 0;
        while i < result.len() {
            for j in 0..i {
                if result[j].states == result[i].states
                    && result[j].is_separator == result[i].is_separator
                    && result[j].precedence == result[i].precedence
                {
                    let characters = mem::take(&mut result[j].characters);
                    result[j].characters = characters.add(&result[i].characters);
                    result.swap_remove(i);
                    i -= 1;
                    break;
                }
            }
            i += 1;
        }

        result.sort_unstable_by(|a, b| a.characters.cmp(&b.characters));
        result
    }

    pub fn completions(&self) -> impl Iterator<Item = (usize, i32)> + '_ {
        self.state_ids.iter().filter_map(move |state_id| {
            if let NfaState::Accept {
                variable_index,
                precedence,
            } = self.nfa.states[*state_id as usize]
            {
                Some((variable_index, precedence))
            } else {
                None
            }
        })
    }

    pub fn add_states(&mut self, new_state_ids: &mut Vec<u32>) {
        let mut i = 0;
        while i < new_state_ids.len() {
            let state_id = new_state_ids[i];
            let state = &self.nfa.states[state_id as usize];
            if let NfaState::Split(left, right) = state {
                let mut has_left = false;
                let mut has_right = false;
                for new_state_id in new_state_ids.iter() {
                    if *new_state_id == *left {
                        has_left = true;
                    }
                    if *new_state_id == *right {
                        has_right = true;
                    }
                }
                if !has_left {
                    new_state_ids.push(*left);
                }
                if !has_right {
                    new_state_ids.push(*right);
                }
            } else if let Err(i) = self.state_ids.binary_search(&state_id) {
                self.state_ids.insert(i, state_id);
            }
            i += 1;
        }
    }
}
