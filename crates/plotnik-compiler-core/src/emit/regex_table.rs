//! Regex table data for bytecode emission.
//!
//! Accumulates pre-compiled sparse-DFA bytes per pattern and builds the regex
//! blob/table sections. Each entry stores both the pattern's StringId (for
//! display in dump/trace) and the serialized DFA (for matching). Pattern
//! *compilation* is the emit-regex pass's job; this type only stores the bytes
//! it is handed, so `core` carries no regex engine dependency.

use std::collections::HashMap;

use plotnik_bytecode::{REGEX_TABLE_ENTRY_SIZE, StringId};

use super::error::EmitError;

/// Compiled regex entry with pattern reference and serialized DFA.
#[derive(Debug)]
struct RegexEntry {
    /// StringId of the pattern (for display in dump/trace).
    string_id: StringId,
    /// Serialized sparse DFA bytes.
    dfa_bytes: Vec<u8>,
}

/// Builds the regex table from pre-compiled DFAs.
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

    /// Store a pre-compiled DFA for `pattern_string_id`, returning its regex ID.
    /// Deduplicates by StringId, so a repeated pattern reuses its first ID.
    pub fn push_dfa(
        &mut self,
        pattern_string_id: StringId,
        dfa_bytes: Vec<u8>,
    ) -> Result<u16, EmitError> {
        if let Some(&id) = self.lookup.get(&pattern_string_id) {
            return Ok(id);
        }

        let id = self.entries.len() as u16;
        if id == u16::MAX {
            return Err(EmitError::TooManyRegexes(self.entries.len()));
        }

        self.entries.push(Some(RegexEntry {
            string_id: pattern_string_id,
            dfa_bytes,
        }));
        self.lookup.insert(pattern_string_id, id);
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

    pub fn lookup(&self, string_id: StringId) -> Option<u16> {
        self.lookup.get(&string_id).copied()
    }

    /// Returns `(blob_bytes, table_bytes)`.
    /// Table entry: `string_id (u16) | reserved (u16) | offset (u32)`.
    pub fn emit(&self) -> (Vec<u8>, Vec<u8>) {
        let mut blob = Vec::new();
        // One entry per pattern plus the trailing sentinel.
        let mut table = Vec::with_capacity((self.entries.len() + 1) * REGEX_TABLE_ENTRY_SIZE);

        for entry in &self.entries {
            // Pad blob to 4-byte alignment before each DFA
            let rem = blob.len() % 4;
            if rem != 0 {
                blob.resize(blob.len() + (4 - rem), 0);
            }

            let (string_id, offset) = if let Some(e) = entry {
                blob.extend_from_slice(&e.dfa_bytes);
                (e.string_id.as_u16(), (blob.len() - e.dfa_bytes.len()) as u32)
            } else {
                (0, 0)
            };

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
