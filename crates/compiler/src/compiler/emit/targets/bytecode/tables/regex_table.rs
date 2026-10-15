use std::collections::HashMap;

use crate::bytecode::{REGEX_TABLE_ENTRY_SIZE, StringId};

use super::error::EmitError;
use super::regex_id::RegexId;

#[derive(Debug)]
struct RegexEntry {
    string_id: StringId,
    dfa_bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct RegexTableBuilder {
    lookup: HashMap<StringId, RegexId>,
    entries: Vec<Option<RegexEntry>>,
}

impl RegexTableBuilder {
    pub fn new() -> Self {
        Self {
            lookup: HashMap::new(),
            entries: vec![None],
        }
    }

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

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn lookup(&self, string_id: StringId) -> Option<RegexId> {
        self.lookup.get(&string_id).copied()
    }

    pub fn emit(&self) -> Result<(Vec<u8>, Vec<u8>), EmitError> {
        let mut blob = Vec::new();
        let mut table = Vec::with_capacity((self.entries.len() + 1) * REGEX_TABLE_ENTRY_SIZE);

        for entry in &self.entries {
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
            table.extend_from_slice(&0u16.to_le_bytes());
            table.extend_from_slice(&offset.to_le_bytes());
        }

        table.extend_from_slice(&0u16.to_le_bytes());
        table.extend_from_slice(&0u16.to_le_bytes());
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
