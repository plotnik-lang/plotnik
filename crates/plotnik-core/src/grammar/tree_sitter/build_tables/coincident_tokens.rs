use std::fmt;

use super::super::{
    bitvec::BitBlock,
    grammars::LexicalGrammar,
    rules::Symbol,
    tables::{ParseStateId, ParseTable},
};

pub struct CoincidentTokenIndex<'a> {
    entries: Vec<Vec<ParseStateId>>,
    /// Flat bitset for fast [`contains()`](Self::contains) checks. Indexed as `a * n + b`
    /// (both `(a,b)` and `(b,a)` bits are set, so no min/max normalization needed).
    contains_bits: Vec<BitBlock>,
    /// Word-aligned per-row bitsets for vectorized intersection checks.
    /// Row `a` spans `[a * row_words .. (a+1) * row_words]`.
    /// Bit `b` is set iff tokens `a` and `b` are coincident in some parse state.
    pub(crate) row_bits: Vec<BitBlock>,
    grammar: &'a LexicalGrammar,
    n: usize,
}

impl<'a> CoincidentTokenIndex<'a> {
    #[must_use]
    pub fn new(table: &ParseTable, lexical_grammar: &'a LexicalGrammar) -> Self {
        Self::build(table, lexical_grammar, true)
    }

    #[must_use]
    pub fn for_metadata(table: &ParseTable, lexical_grammar: &'a LexicalGrammar) -> Self {
        Self::build(table, lexical_grammar, false)
    }

    fn build(table: &ParseTable, lexical_grammar: &'a LexicalGrammar, build_rows: bool) -> Self {
        let n = lexical_grammar.variables.len();
        let bits_per_word = BitBlock::BITS as usize;
        let row_words = n.div_ceil(bits_per_word);
        let mut result = Self {
            n,
            grammar: lexical_grammar,
            entries: vec![Vec::new(); n * n],
            contains_bits: vec![BitBlock::default(); (n * n).div_ceil(bits_per_word)],
            row_bits: if build_rows {
                vec![BitBlock::default(); n * row_words]
            } else {
                Vec::new()
            },
        };
        // Pre-collect terminal indices up front rather than continuously recomputing within the
        // loop below.
        let mut terminal_indices = Vec::new();
        for (i, state) in table.states.iter().enumerate() {
            terminal_indices.clear();
            terminal_indices.extend(
                state
                    .terminal_entries
                    .keys()
                    .filter(|s| s.is_terminal())
                    .map(|s| s.index),
            );
            for (j, &a) in terminal_indices.iter().enumerate() {
                for &b in &terminal_indices[j..] {
                    let index = result.index(a, b);
                    if result.entries[index].last().copied() != Some(i) {
                        result.entries[index].push(i);
                    }
                    // Set both (a,b) and (b,a) bits so `contains()` needs
                    // no min/max normalization.
                    let ab = a * n + b;
                    result.contains_bits[ab / bits_per_word] |=
                        BitBlock::from(1u8) << (ab % bits_per_word);
                    let ba = b * n + a;
                    result.contains_bits[ba / bits_per_word] |=
                        BitBlock::from(1u8) << (ba % bits_per_word);
                    if build_rows {
                        result.row_bits[a * row_words + b / bits_per_word] |=
                            BitBlock::from(1u8) << (b % bits_per_word);
                        result.row_bits[b * row_words + a / bits_per_word] |=
                            BitBlock::from(1u8) << (a % bits_per_word);
                    }
                }
            }
        }
        result
    }

    #[must_use]
    pub fn states_with(&self, a: Symbol, b: Symbol) -> &[ParseStateId] {
        &self.entries[self.index(a.index, b.index)]
    }

    #[must_use]
    pub fn contains(&self, a: Symbol, b: Symbol) -> bool {
        let bit_index = a.index * self.n + b.index;
        let bits_per_word = BitBlock::BITS as usize;
        self.contains_bits[bit_index / bits_per_word]
            & (BitBlock::from(1u8) << (bit_index % bits_per_word))
            != 0
    }

    #[must_use]
    const fn index(&self, a: usize, b: usize) -> usize {
        if a < b {
            a * self.n + b
        } else {
            b * self.n + a
        }
    }
}

impl fmt::Debug for CoincidentTokenIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "CoincidentTokenIndex {{")?;
        for i in 0..self.n {
            let mut coincident = Vec::new();
            for j in 0..self.n {
                if self.contains(Symbol::terminal(i), Symbol::terminal(j)) {
                    coincident.push(&self.grammar.variables[j].name);
                }
            }
            if !coincident.is_empty() {
                writeln!(f, "  {}: {:?},", self.grammar.variables[i].name, coincident)?;
            }
        }
        write!(f, "}}")?;
        Ok(())
    }
}
