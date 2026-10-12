//! Regex table data for bytecode emission.
//!
//! Accumulates pre-compiled sparse-DFA bytes per pattern and builds the regex
//! blob/table sections. Each entry stores both the pattern's StringId (for
//! display in dump/trace) and the serialized DFA (for matching). Pattern
//! *compilation* is the emit-regex pass's job; this type only stores the bytes
//! it is handed, so `core` carries no regex engine dependency.

use std::collections::HashMap;

use crate::bytecode::{REGEX_TABLE_ENTRY_SIZE, StringId};

use super::error::EmitError;
use super::regex_id::RegexId;

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
#[derive(Debug)]
pub struct RegexTableBuilder {
    /// Map from StringId to regex ID (for deduplication).
    lookup: HashMap<StringId, RegexId>,
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
    ) -> Result<RegexId, EmitError> {
        if let Some(&id) = self.lookup.get(&pattern_string_id) {
            return Ok(id);
        }

        if self.entries.len() >= EmitError::MAX_REGEXES {
            return Err(EmitError::TooManyRegexes(self.entries.len() + 1));
        }

        let id = RegexId::try_from(self.entries.len()).expect("regex count checked");
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

    pub fn lookup(&self, string_id: StringId) -> Option<RegexId> {
        self.lookup.get(&string_id).copied()
    }

    /// Returns `(blob_bytes, table_bytes)`.
    /// Table entry: `string_id (u16) | reserved (u16) | offset (u32)`.
    pub fn emit(&self) -> Result<(Vec<u8>, Vec<u8>), EmitError> {
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
                let offset = blob_offset(blob.len())?;
                blob.extend_from_slice(&e.dfa_bytes);
                (u16::from(e.string_id), offset)
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
        table.extend_from_slice(&blob_offset(blob.len())?.to_le_bytes());

        Ok((blob, table))
    }
}

fn blob_offset(size: usize) -> Result<u32, EmitError> {
    u32::try_from(size).map_err(|_| EmitError::SectionTooLarge {
        section: "regex blob",
        size,
    })
}
