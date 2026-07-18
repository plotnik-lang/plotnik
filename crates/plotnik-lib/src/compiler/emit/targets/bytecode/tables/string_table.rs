//! String table builder for bytecode emission.
//!
//! Builds the string table section, remapping query Symbols to bytecode StringIds.

use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{Interner, Symbol};

use crate::bytecode::StringId;

use super::error::EmitError;

/// Easter egg string at index 0 (Dostoevsky, The Idiot).
/// StringId(0) is reserved and never referenced by instructions.
pub const EASTER_EGG: &str = "Beauty will save the world";

/// Builds a subset of the query interner's strings into bytecode StringIds.
/// This builder collects only the strings that are actually used and assigns
/// compact StringId indices.
///
/// StringId(0) is reserved for an easter egg and is never referenced by
/// instructions. Actual strings start at index 1.
#[derive(Debug)]
pub struct StringTableBuilder {
    /// Map from query Symbol to bytecode StringId.
    mapping: HashMap<Symbol, StringId>,
    /// Reverse lookup from string content to StringId (for intern_str). Shares
    /// each string's allocation with `strings` via `Rc`.
    str_lookup: HashMap<Rc<str>, StringId>,
    /// Ordered strings for the binary.
    strings: Vec<Rc<str>>,
}

impl StringTableBuilder {
    pub fn new() -> Self {
        let mut builder = Self {
            mapping: HashMap::new(),
            str_lookup: HashMap::new(),
            strings: Vec::new(),
        };
        // Reserve index 0 for easter egg (never looked up via str_lookup)
        builder.strings.push(Rc::from(EASTER_EGG));
        builder
    }

    pub fn intern(&mut self, sym: Symbol, interner: &Interner) -> Result<StringId, EmitError> {
        if let Some(&id) = self.mapping.get(&sym) {
            return Ok(id);
        }

        let text = interner.try_resolve(sym).unwrap_or_else(|| {
            panic!(
                "bytecode string-table emission received symbol {sym:?}, but the query \
                     interner cannot resolve it"
            )
        });

        let id = self.intern_str(text)?;
        self.mapping.insert(sym, id);
        Ok(id)
    }

    /// Intern a string directly (for generated strings not in the query interner).
    pub fn intern_str(&mut self, s: &str) -> Result<StringId, EmitError> {
        if let Some(&id) = self.str_lookup.get(s) {
            return Ok(id);
        }

        if self.strings.len() >= EmitError::MAX_STRINGS {
            return Err(EmitError::TooManyStrings(self.strings.len() + 1));
        }

        let id = StringId::try_from(self.strings.len() as u16).expect("string id must be non-zero");
        let s: Rc<str> = Rc::from(s);
        self.strings.push(Rc::clone(&s));
        self.str_lookup.insert(s, id);
        Ok(id)
    }

    /// Number of interned strings.
    pub fn len(&self) -> usize {
        self.strings.len()
    }

    /// Validate that the string count fits in u16.
    pub fn validate(&self) -> Result<(), EmitError> {
        if self.strings.len() > EmitError::MAX_STRINGS {
            return Err(EmitError::TooManyStrings(self.strings.len()));
        }
        Ok(())
    }

    /// Get the StringId for direct string content, if it was interned.
    pub fn lookup_str(&self, s: &str) -> Option<StringId> {
        self.str_lookup.get(s).copied()
    }

    /// Returns `(blob_bytes, table_bytes)`.
    pub fn emit(&self) -> Result<(Vec<u8>, Vec<u8>), EmitError> {
        let mut blob = Vec::new();
        let mut offsets: Vec<u32> = Vec::with_capacity(self.strings.len() + 1);

        for s in &self.strings {
            offsets.push(blob_offset(blob.len())?);
            blob.extend_from_slice(s.as_bytes());
        }
        offsets.push(blob_offset(blob.len())?); // sentinel

        let table_bytes: Vec<u8> = offsets.iter().flat_map(|o| o.to_le_bytes()).collect();

        Ok((blob, table_bytes))
    }
}

fn blob_offset(size: usize) -> Result<u32, EmitError> {
    u32::try_from(size).map_err(|_| EmitError::SectionTooLarge {
        section: "string blob",
        size,
    })
}

impl Default for StringTableBuilder {
    fn default() -> Self {
        Self::new()
    }
}
