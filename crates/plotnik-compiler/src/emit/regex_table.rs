//! Regex table builder for bytecode emission.
//!
//! Compiles regex patterns to sparse DFAs and builds the regex blob/table sections.
//! Each entry stores both the pattern's StringId (for display) and DFA offset (for matching).

use std::collections::HashMap;

use regex_automata::dfa::dense;
use regex_automata::dfa::sparse::DFA;

use plotnik_bytecode::StringId;

use super::EmitError;

/// Compiled regex entry with pattern reference and serialized DFA.
#[derive(Debug)]
struct RegexEntry {
    /// StringId of the pattern (for display in dump/trace).
    string_id: StringId,
    /// Serialized sparse DFA bytes.
    dfa_bytes: Vec<u8>,
}

/// Builds the regex table, compiling patterns to sparse DFAs.
///
/// Each regex is compiled to a sparse DFA and serialized. The table stores
/// both StringId (for pattern display) and offset into the blob (for DFA access).
///
/// Index 0 is unused (regex_id 0 means "no regex").
#[derive(Debug, Default)]
pub struct RegexTableBuilder {
    /// Map from StringId to regex ID (for deduplication).
    lookup: HashMap<StringId, u16>,
    /// Compiled regex entries (index 0 is unused).
    entries: Vec<Option<RegexEntry>>,
}

impl RegexTableBuilder {
    pub fn new() -> Self {
        Self {
            lookup: HashMap::new(),
            entries: vec![None], // index 0 reserved
        }
    }

    /// Intern a regex pattern, compiling it to a DFA.
    ///
    /// Takes the pattern string and its StringId. Returns the regex ID on success.
    pub fn intern(&mut self, pattern: &str, string_id: StringId) -> Result<u16, EmitError> {
        if let Some(&id) = self.lookup.get(&string_id) {
            return Ok(id);
        }

        // Compile to dense DFA first, then convert to sparse
        let dense = dense::DFA::builder()
            .configure(
                dense::DFA::config()
                    .start_kind(regex_automata::dfa::StartKind::Unanchored)
                    .minimize(true),
            )
            .build(pattern)
            .map_err(|e| EmitError::RegexCompile(pattern.to_string(), e.to_string()))?;

        let sparse = dense
            .to_sparse()
            .map_err(|e| EmitError::RegexCompile(pattern.to_string(), e.to_string()))?;

        let dfa_bytes = sparse.to_bytes_little_endian();

        let id = self.entries.len() as u16;
        if id == u16::MAX {
            return Err(EmitError::TooManyRegexes(self.entries.len()));
        }

        self.entries.push(Some(RegexEntry {
            string_id,
            dfa_bytes,
        }));
        self.lookup.insert(string_id, id);
        Ok(id)
    }

    /// Number of regexes (including reserved index 0).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the builder has any regexes (beyond reserved index 0).
    pub fn is_empty(&self) -> bool {
        self.entries.len() <= 1
    }

    /// Validate that the regex count fits in u16.
    pub fn validate(&self) -> Result<(), EmitError> {
        if self.entries.len() > 65535 {
            return Err(EmitError::TooManyRegexes(self.entries.len()));
        }
        Ok(())
    }

    /// Lookup a regex ID by StringId.
    pub fn get(&self, string_id: StringId) -> Option<u16> {
        self.lookup.get(&string_id).copied()
    }

    /// Emit the regex blob and table.
    ///
    /// Returns (blob_bytes, table_bytes).
    ///
    /// Table format per entry: `string_id (u16) | reserved (u16) | offset (u32)`
    /// This allows looking up both the pattern string (via StringTable) and DFA bytes.
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>) {
        let mut blob = Vec::new();
        let mut table = Vec::with_capacity(self.entries.len() * 8 + 4);

        for entry in &self.entries {
            // Pad blob to 4-byte alignment before each DFA
            let rem = blob.len() % 4;
            if rem != 0 {
                blob.resize(blob.len() + (4 - rem), 0);
            }

            let (string_id, offset) = if let Some(e) = entry {
                blob.extend_from_slice(&e.dfa_bytes);
                (e.string_id.get(), (blob.len() - e.dfa_bytes.len()) as u32)
            } else {
                (0, 0)
            };

            // Emit table entry: string_id (u16) | reserved (u16) | offset (u32)
            table.extend_from_slice(&string_id.to_le_bytes());
            table.extend_from_slice(&0u16.to_le_bytes()); // reserved
            table.extend_from_slice(&offset.to_le_bytes());
        }

        // Sentinel entry with blob end offset
        table.extend_from_slice(&0u16.to_le_bytes()); // string_id = 0
        table.extend_from_slice(&0u16.to_le_bytes()); // reserved
        table.extend_from_slice(&(blob.len() as u32).to_le_bytes());

        (blob, table)
    }
}

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
