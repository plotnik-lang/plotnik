use std::{
    char,
    cmp::Ordering,
    fmt,
    iter::ExactSizeIterator,
    mem::swap,
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

const END: u32 = char::MAX as u32 + 1;

impl CharacterSet {
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
    pub fn add_range(mut self, start: char, end: char) -> Self {
        self.add_int_range(0, start as u32, end as u32 + 1);
        self
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
