use std::collections::HashMap;
use std::rc::Rc;

use crate::core::{Interner, Symbol};

use crate::bytecode::StringId;

use super::error::EmitError;

pub const EASTER_EGG: &str = "Beauty will save the world";

#[derive(Debug)]
pub struct StringTableBuilder {
    mapping: HashMap<Symbol, StringId>,
    str_lookup: HashMap<Rc<str>, StringId>,
    strings: Vec<Rc<str>>,
}

impl StringTableBuilder {
    pub fn new() -> Self {
        let mut builder = Self {
            mapping: HashMap::new(),
            str_lookup: HashMap::new(),
            strings: Vec::new(),
        };
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

    pub fn len(&self) -> usize {
        self.strings.len()
    }

    pub fn lookup_str(&self, s: &str) -> Option<StringId> {
        self.str_lookup.get(s).copied()
    }

    pub fn emit(&self) -> Result<(Vec<u8>, Vec<u8>), EmitError> {
        let mut blob = Vec::new();
        let mut offsets: Vec<u32> = Vec::with_capacity(self.strings.len() + 1);

        for s in &self.strings {
            offsets.push(blob_offset(blob.len())?);
            blob.extend_from_slice(s.as_bytes());
        }
        offsets.push(blob_offset(blob.len())?);

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
