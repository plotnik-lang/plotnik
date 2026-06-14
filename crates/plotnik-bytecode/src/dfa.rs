//! Sparse DFA storage and deserialization for regex predicates.

use regex_automata::Input;
use regex_automata::dfa::Automaton;
use regex_automata::dfa::sparse::DFA;

/// Deserialize a sparse DFA from bytecode, borrowing `bytes`.
///
/// `DFA::from_bytes` validates the entire serialized automaton, so this doubles
/// as the load-time gate that proves a module's regex blob is well-formed.
/// [`RegexDfas`] keeps the validated automaton instead of re-deserializing it.
///
/// # Safety
/// The bytes must have been produced by `DFA::to_bytes_little_endian()`.
pub fn deserialize_dfa(bytes: &[u8]) -> Result<DFA<&[u8]>, String> {
    // SAFETY: We only serialize DFAs we built, and the format is stable
    // within the same regex-automata version.
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
/// These owned automata are built once, folded into the load-time validation
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
    /// Wrap per-index owned DFAs (slot 0 is the reserved sentinel).
    pub(crate) fn new(dfas: Vec<Option<DFA<Vec<u8>>>>) -> Self {
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
