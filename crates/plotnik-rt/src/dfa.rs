//! Sparse DFA storage and deserialization for regex predicates.

use std::sync::OnceLock;

use regex_automata::Input;
use regex_automata::dfa::Automaton;
use regex_automata::dfa::sparse::DFA;

/// `DFA::from_bytes` validates the entire serialized automaton (no unsafe
/// trust involved), so this doubles as the load-time gate that proves a
/// module's regex blob is well-formed. [`RegexDfas`] keeps the validated
/// automaton instead of re-deserializing it.
///
/// The compiler serializes with `to_bytes_little_endian`, and the validation
/// also rejects an endianness or regex-automata version mismatch — which is
/// how a big-endian target or a skewed dependency graph surfaces: as an
/// `Err` here, not as silent misbehavior.
pub fn deserialize_dfa(bytes: &[u8]) -> Result<DFA<&[u8]>, String> {
    DFA::from_bytes(bytes)
        .map(|(dfa, _)| dfa)
        .map_err(|e| e.to_string())
}

/// A module's regex-predicate DFAs, deserialized once at load and reused on
/// every evaluation (issue #426).
///
/// `DFA::from_bytes` re-validates the whole serialized automaton, so the old
/// path — deserializing inside the VM's match loop — re-paid that cost on every
/// predicate test, thousands of times over a quantified pattern on a large file.
/// These owned automata are built once, folded into load-time validation
/// that already deserialized each DFA, then searched directly thereafter. The
/// owned copy duplicates bytes still resident in the module's serialized blob —
/// that blob is one immutable, contiguous allocation backing every section, so
/// the duplication is inherent rather than separately reclaimable.
///
/// Indexed in parallel with the regex table: slot 0 is the reserved sentinel
/// (`None`); every real pattern `1..count` holds its owned DFA.
#[derive(Default)]
pub struct RegexDfas {
    dfas: Vec<Option<DFA<Vec<u8>>>>,
}

impl RegexDfas {
    pub fn new(dfas: Vec<Option<DFA<Vec<u8>>>>) -> Self {
        Self { dfas }
    }

    /// Whether the pattern at `idx` matches anywhere in `text`.
    ///
    /// `idx` is a predicate operand the loader bounds to a real entry
    /// (`1..count`, see `Module::validate_extended_match`), so the slot is always
    /// populated and the search cannot fail — an empty slot or a search error
    /// would be a forged-/miscompiled-module bug, stated loudly here rather than
    /// silently mis-answered.
    pub fn is_match(&self, idx: usize, text: &str) -> bool {
        let dfa = self.dfas[idx]
            .as_ref()
            .expect("regex predicate references reserved DFA slot");
        dfa.try_search_fwd(&Input::new(text))
            .expect("regex DFA search failed")
            .is_some()
    }
}

impl std::fmt::Debug for RegexDfas {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The inner sparse DFAs are not `Debug`; the slot count is enough to keep
        // the enclosing `Module`'s derived `Debug` meaningful.
        f.debug_struct("RegexDfas")
            .field("count", &self.dfas.len())
            .finish()
    }
}

/// A regex predicate embedded in generated code as serialized sparse-DFA
/// bytes — the generated-matcher analogue of [`RegexDfas`]' load-once
/// discipline. `DFA::from_bytes` validates the whole automaton, so the first
/// use doubles as the validation gate; every later search reuses the
/// deserialized automaton.
pub struct StaticDfa {
    bytes: &'static [u8],
    dfa: OnceLock<DFA<&'static [u8]>>,
}

impl StaticDfa {
    /// `bytes` must come from the compiler's own DFA serialization (the emit
    /// pipeline's `to_bytes_little_endian`); generated code bakes them as a
    /// static.
    pub const fn new(bytes: &'static [u8]) -> Self {
        Self {
            bytes,
            dfa: OnceLock::new(),
        }
    }

    /// Whether the pattern matches anywhere in `text`.
    pub fn is_match(&self, text: &str) -> bool {
        let dfa = self.dfa.get_or_init(|| {
            deserialize_dfa(self.bytes).unwrap_or_else(|error| {
                panic!(
                    "embedded regex DFA failed to load: {error}; the bytes were \
                     serialized little-endian at generation time, so a big-endian \
                     target or a regex-automata version skew between compiler and \
                     runtime cannot run this module"
                )
            })
        });
        dfa.try_search_fwd(&Input::new(text))
            .expect("regex DFA search failed")
            .is_some()
    }
}

impl std::fmt::Debug for StaticDfa {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticDfa")
            .field("bytes", &self.bytes.len())
            .finish()
    }
}
