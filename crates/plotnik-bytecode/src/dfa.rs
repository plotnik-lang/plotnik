//! DFA deserialization for regex predicates.
//!
//! Extracted from emit for use by the runtime.

use regex_automata::dfa::sparse::DFA;

/// Deserialize a sparse DFA from bytecode.
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
