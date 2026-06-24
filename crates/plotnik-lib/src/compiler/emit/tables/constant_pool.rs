//! Read-only view over sealed bytecode constant tables.

use crate::bytecode::StringId;
use crate::compiler::ids::TypeId;

use super::{RegexTableBuilder, StringTableBuilder, TypeTableBuilder};

#[derive(Clone, Copy)]
pub(in crate::compiler::emit) struct ConstantPool<'a> {
    types: &'a TypeTableBuilder,
    strings: &'a StringTableBuilder,
    regexes: &'a RegexTableBuilder,
}

impl<'a> ConstantPool<'a> {
    pub(in crate::compiler::emit) fn new(
        types: &'a TypeTableBuilder,
        strings: &'a StringTableBuilder,
        regexes: &'a RegexTableBuilder,
    ) -> Self {
        Self {
            types,
            strings,
            regexes,
        }
    }

    pub(in crate::compiler::emit) fn lookup_str(self, value: &str) -> Option<StringId> {
        self.strings.lookup_str(value)
    }

    pub(in crate::compiler::emit) fn lookup_regex(self, string_id: StringId) -> Option<u16> {
        self.regexes.lookup(string_id)
    }

    pub(in crate::compiler::emit) fn member_base(self, type_id: TypeId) -> Option<u16> {
        self.types.get_member_base(type_id)
    }

    pub(in crate::compiler::emit) fn emit_strings(self) -> (Vec<u8>, Vec<u8>) {
        self.strings.emit()
    }

    pub(in crate::compiler::emit) fn emit_regexes(self) -> (Vec<u8>, Vec<u8>) {
        self.regexes.emit()
    }

    pub(in crate::compiler::emit) fn emit_types(self) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        self.types.emit()
    }

    pub(in crate::compiler::emit) fn string_count(self) -> usize {
        self.strings.len()
    }

    pub(in crate::compiler::emit) fn regex_count(self) -> usize {
        self.regexes.len()
    }

    pub(in crate::compiler::emit) fn type_defs_count(self) -> usize {
        self.types.type_defs_count()
    }

    pub(in crate::compiler::emit) fn type_members_count(self) -> usize {
        self.types.type_members_count()
    }

    pub(in crate::compiler::emit) fn type_names_count(self) -> usize {
        self.types.type_names_count()
    }
}
