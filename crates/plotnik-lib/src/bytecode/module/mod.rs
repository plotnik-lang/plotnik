//! Bytecode module with unified storage.
//!
//! [`Module`] holds compiled bytecode plus a pre-decoded instruction stream.
//! Construction remains crate-private so only compiler output can cross the
//! checked loader boundary.

use std::ops::Deref;

use super::aligned_vec::AlignedVec;
use super::header::{Header, SectionOffsets};
use super::ids::{StringId, TypeId};
use super::instructions::{Call, Match, Opcode, Return, RoutedCall, SplitCall, header_byte};
use super::sections::SymbolNameEntry;
use super::spans::SpansView;
use super::type_meta::{TypeDef, TypeDefKind, TypeKind, TypeMember, TypeNameEntry};
use super::{
    BYTECODE_WORD_SIZE, CodeAddr, EntryPoint, REGEX_TABLE_ENTRY_SIZE, SPAN_ENTRY_SIZE,
    STRING_TABLE_ENTRY_SIZE,
};
use plotnik_rt::RegexDfas;

mod decoded;
mod effect_stack;
mod load;

pub(crate) use decoded::{
    DecodedCall, DecodedInstr, DecodedMatch, DecodedPredicate, DecodedProgram, DecodedRoutedCall,
    DecodedSplitCall,
};
pub(crate) use load::ModuleError;

#[inline]
fn read_u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

#[inline]
fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Compiler-owned bytecode storage with guaranteed 64-byte alignment.
pub(crate) struct ByteStorage(AlignedVec);

impl Deref for ByteStorage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::fmt::Debug for ByteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ByteStorage").field(&self.0.len()).finish()
    }
}

impl ByteStorage {
    /// Copy compiler-emitted bytes into the aligned runtime buffer.
    pub(crate) fn from_emitted_bytes(bytes: &[u8]) -> Self {
        Self(AlignedVec::copy_from_slice(bytes))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Instruction<'a> {
    Match(Match<'a>),
    Call(Call),
    RoutedCall(RoutedCall),
    SplitCall(SplitCall),
    Return(Return),
}

impl<'a> Instruction<'a> {
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        debug_assert!(bytes.len() >= BYTECODE_WORD_SIZE, "instruction too short");

        let opcode = header_byte::opcode(bytes[0]).expect("invalid opcode");
        match opcode {
            Opcode::Call => {
                let arr: [u8; 8] = bytes[..8].try_into().expect("slice is exactly 8 bytes");
                Self::Call(Call::from_bytes(arr))
            }
            Opcode::SplitCall => {
                let arr: [u8; 8] = bytes[..8].try_into().expect("slice is exactly 8 bytes");
                Self::SplitCall(SplitCall::from_bytes(arr))
            }
            Opcode::RoutedCall => {
                let arr: [u8; 8] = bytes[..8].try_into().expect("slice is exactly 8 bytes");
                Self::RoutedCall(RoutedCall::from_bytes(arr))
            }
            Opcode::Return => {
                let arr: [u8; 8] = bytes[..8].try_into().expect("slice is exactly 8 bytes");
                Self::Return(Return::from_bytes(arr))
            }
            _ => Self::Match(Match::from_bytes(bytes)),
        }
    }
}

/// A compiled bytecode module.
///
/// Instructions are decoded lazily via [`decode_instruction`](Self::decode_instruction).
/// Cold data (strings, symbols, types) is accessed through view methods.
#[derive(Debug)]
pub struct Module {
    storage: ByteStorage,
    header: Header,
    /// Cached section offsets (computed from header counts).
    offsets: SectionOffsets,
    /// Regex-predicate DFAs, deserialized once at module load and reused by the
    /// VM on every evaluation instead of being rebuilt from the blob each time
    /// (issue #426).
    regex_dfas: RegexDfas,
    /// Pre-decoded instructions, built at module load after validation (the hot loop
    /// indexes this instead of re-parsing bytes; see `decoded`).
    decoded: DecodedProgram,
    /// Per-word "is an instruction start" bitmap from load validation
    /// ([`validate_instructions`](Self::validate_instructions)), retained only in
    /// debug builds to back the VM's pre-decode IP assertion. It does not
    /// exist in release, so the steady-state module carries no extra memory.
    #[cfg(debug_assertions)]
    instr_start_bitmap: Vec<bool>,
}

impl Module {
    /// Load compiler output into the VM after running every boundary check.
    ///
    /// Crate-private visibility keeps this loader on the compiler-to-VM boundary.
    pub(crate) fn load_compiler_output(bytes: &[u8]) -> Result<Self, ModuleError> {
        Self::load_storage(ByteStorage::from_emitted_bytes(bytes))
    }

    pub(crate) fn header(&self) -> &Header {
        &self.header
    }

    #[cfg(test)]
    pub(crate) fn offsets(&self) -> &SectionOffsets {
        &self.offsets
    }

    #[cfg(test)]
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.storage
    }

    /// Size of the bytecode module, for diagnostics and teaching tools.
    pub fn bytecode_size(&self) -> usize {
        self.storage.len()
    }

    #[inline]
    pub(crate) fn decode_instruction(&self, addr: CodeAddr) -> Instruction<'_> {
        let offset = self.offsets.instructions as usize + addr.as_usize() * BYTECODE_WORD_SIZE;
        Instruction::from_bytes(&self.storage[offset..])
    }

    #[inline]
    pub(crate) fn decoded(&self) -> &DecodedProgram {
        &self.decoded
    }

    /// Whether `addr` is a validated instruction start.
    ///
    /// Backs the VM's pre-decode IP assertion, localizing a bad jump to the address
    /// that wrote `ip` rather than letting [`decode_instruction`](Self::decode_instruction)
    /// begin mid-instruction. Debug-only: the backing bitmap is retained at load
    /// under `debug_assertions` and does not exist in release.
    #[cfg(debug_assertions)]
    pub fn is_validated_instruction_start(&self, addr: CodeAddr) -> bool {
        self.instr_start_bitmap
            .get(addr.as_usize())
            .copied()
            .unwrap_or(false)
    }

    pub fn strings(&self) -> StringsView<'_> {
        StringsView {
            blob: &self.storage[self.offsets.str_blob as usize..],
            table: self.string_table_slice(),
        }
    }

    pub(crate) fn node_kinds(&self) -> GrammarTableView<'_> {
        let offset = self.offsets.node_kinds as usize;
        let count = self.header.node_kinds_count as usize;
        GrammarTableView {
            bytes: &self.storage[offset..offset + count * SymbolNameEntry::SIZE],
            count,
        }
    }

    pub(crate) fn node_fields(&self) -> GrammarTableView<'_> {
        let offset = self.offsets.node_fields as usize;
        let count = self.header.node_fields_count as usize;
        GrammarTableView {
            bytes: &self.storage[offset..offset + count * SymbolNameEntry::SIZE],
            count,
        }
    }

    pub(crate) fn regexes(&self) -> RegexView<'_> {
        RegexView {
            blob: &self.storage[self.offsets.regex_blob as usize..],
            table: self.regex_table_slice(),
        }
    }

    /// Regex-predicate DFAs, deserialized once at load (issue #426).
    ///
    /// The VM evaluates `=~`/`!~` against these cached automata; rebuilding them
    /// from [`regexes`](Self::regexes)'s raw blob per call is what this avoids.
    pub(crate) fn regex_dfas(&self) -> &RegexDfas {
        &self.regex_dfas
    }

    pub fn types(&self) -> TypesView<'_> {
        let defs_offset = self.offsets.type_defs as usize;
        let defs_count = self.header.type_defs_count as usize;
        let members_offset = self.offsets.type_members as usize;
        let members_count = self.header.type_members_count as usize;
        let names_offset = self.offsets.type_names as usize;
        let names_count = self.header.type_names_count as usize;

        TypesView {
            defs_bytes: &self.storage[defs_offset..defs_offset + defs_count * TypeDef::SIZE],
            members_bytes: &self.storage
                [members_offset..members_offset + members_count * TypeMember::SIZE],
            names_bytes: &self.storage
                [names_offset..names_offset + names_count * TypeNameEntry::SIZE],
            defs_count,
            members_count,
            names_count,
        }
    }

    pub fn entry_points(&self) -> EntryPointsView<'_> {
        let offset = self.offsets.entrypoints as usize;
        let count = self.header.entrypoints_count as usize;
        EntryPointsView {
            bytes: &self.storage[offset..offset + count * EntryPoint::SIZE],
            count,
        }
    }

    pub fn spans(&self) -> SpansView<'_> {
        SpansView::new(self.spans_slice(), self.header.spans_count as usize)
    }

    pub fn entry_point_count(&self) -> usize {
        self.header.entrypoints_count as usize
    }

    pub fn entry_point_at(&self, idx: usize) -> Option<EntryPoint> {
        (idx < self.entry_point_count()).then(|| self.entry_points().get(idx))
    }

    pub fn entry_point(&self, name: &str) -> Option<EntryPoint> {
        self.entry_points().find_by_name(name, &self.strings())
    }

    /// Names of all entrypoints, in table order.
    pub fn entry_point_names(&self) -> impl Iterator<Item = &str> {
        let strings = self.strings();
        let entrypoints = self.entry_points();
        (0..self.entry_point_count()).map(move |i| strings.get(entrypoints.get(i).name()))
    }

    /// `count + 1` entries: the extra sentinel offset gives the final string's end.
    fn string_table_slice(&self) -> &[u8] {
        let offset = self.offsets.str_table as usize;
        let count = self.header.str_table_count as usize;
        &self.storage[offset..offset + (count + 1) * STRING_TABLE_ENTRY_SIZE]
    }

    /// `count + 1` entries: the extra sentinel offset gives the final DFA's end.
    fn regex_table_slice(&self) -> &[u8] {
        let offset = self.offsets.regex_table as usize;
        let count = self.header.regex_table_count as usize;
        &self.storage[offset..offset + (count + 1) * REGEX_TABLE_ENTRY_SIZE]
    }

    fn instructions_slice(&self) -> &[u8] {
        let offset = self.offsets.instructions as usize;
        let len = self.header.instruction_word_count as usize * BYTECODE_WORD_SIZE;
        &self.storage[offset..offset + len]
    }

    fn spans_slice(&self) -> &[u8] {
        let offset = self.offsets.spans as usize;
        let len = self.header.spans_count as usize * SPAN_ENTRY_SIZE;
        &self.storage[offset..offset + len]
    }
}

/// View into the string table.
pub struct StringsView<'a> {
    blob: &'a [u8],
    table: &'a [u8],
}

impl<'a> StringsView<'a> {
    pub fn get(&self, id: StringId) -> &'a str {
        self.at(u16::from(id) as usize)
    }

    /// Get a string by raw index (for iteration/dumps, including easter egg at 0).
    ///
    /// The string table contains sequential u32 offsets. To get string i:
    /// `start = table[i]`, `end = table[i+1]`, `length = end - start`.
    pub fn at(&self, idx: usize) -> &'a str {
        let start = read_u32_le(self.table, idx * STRING_TABLE_ENTRY_SIZE) as usize;
        let end = read_u32_le(self.table, (idx + 1) * STRING_TABLE_ENTRY_SIZE) as usize;
        std::str::from_utf8(&self.blob[start..end]).expect("invalid UTF-8 in string table")
    }
}

pub struct GrammarTableView<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> GrammarTableView<'a> {
    pub fn get(&self, idx: usize) -> SymbolNameEntry {
        assert!(idx < self.count, "symbol-name table index out of bounds");
        let offset = idx * SymbolNameEntry::SIZE;
        SymbolNameEntry::new(
            read_u16_le(self.bytes, offset),
            StringId::try_from(read_u16_le(self.bytes, offset + 2))
                .expect("symbol name id must be non-zero"),
        )
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = SymbolNameEntry> + '_ {
        (0..self.count).map(|idx| self.get(idx))
    }
}

/// View into the regex table.
///
/// Entry layout: `string_id (u16) | reserved (u16) | offset (u32)` = 8 bytes.
pub struct RegexView<'a> {
    blob: &'a [u8],
    table: &'a [u8],
}

impl<'a> RegexView<'a> {
    const ENTRY_SIZE: usize = REGEX_TABLE_ENTRY_SIZE;

    /// Raw serialized DFA bytes; use `regex-automata` to deserialize: `DFA::from_bytes(&bytes)`.
    pub fn at(&self, idx: usize) -> &'a [u8] {
        let entry_offset = idx * Self::ENTRY_SIZE;
        let next_entry_offset = (idx + 1) * Self::ENTRY_SIZE;

        let start = read_u32_le(self.table, entry_offset + 4) as usize;
        let end = read_u32_le(self.table, next_entry_offset + 4) as usize;
        &self.blob[start..end]
    }

    /// Pattern `StringId` for display (e.g. `dump`/`trace`).
    pub fn pattern_string_id(&self, idx: usize) -> super::StringId {
        let entry_offset = idx * Self::ENTRY_SIZE;
        let string_id = read_u16_le(self.table, entry_offset);
        super::StringId::try_from(string_id).expect("regex pattern string id must be non-zero")
    }
}

/// View into type metadata.
///
/// Types are stored in three sub-sections:
/// - TypeDefs: structural topology (4 bytes each)
/// - TypeMembers: fields and variants (4 bytes each)
/// - TypeNames: name → TypeId mapping (4 bytes each)
pub struct TypesView<'a> {
    defs_bytes: &'a [u8],
    members_bytes: &'a [u8],
    names_bytes: &'a [u8],
    defs_count: usize,
    members_count: usize,
    names_count: usize,
}

impl<'a> TypesView<'a> {
    pub fn def(&self, idx: usize) -> TypeDef {
        assert!(idx < self.defs_count, "type def index out of bounds");
        let offset = idx * TypeDef::SIZE;
        TypeDef::from_bytes(&self.defs_bytes[offset..])
    }

    pub fn get(&self, id: TypeId) -> Option<TypeDef> {
        let idx = u16::from(id) as usize;
        if idx < self.defs_count {
            Some(self.def(idx))
        } else {
            None
        }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = TypeDef> + '_ {
        (0..self.defs_count).map(|idx| self.def(idx))
    }

    pub fn get_member(&self, idx: usize) -> TypeMember {
        assert!(idx < self.members_count, "type member index out of bounds");
        let offset = idx * TypeMember::SIZE;
        TypeMember::new(
            StringId::try_from(read_u16_le(self.members_bytes, offset))
                .expect("type member name id must be non-zero"),
            TypeId::from(read_u16_le(self.members_bytes, offset + 2)),
        )
    }

    pub fn members(&self) -> impl ExactSizeIterator<Item = TypeMember> + '_ {
        (0..self.members_count).map(|idx| self.get_member(idx))
    }

    /// A member's `type_id` without building the (`NonZero`) name `StringId`.
    /// Load-time validation uses this so a malformed zero name cannot panic the
    /// validator before `validate_string_ids` rejects it.
    pub(crate) fn member_type_id(&self, idx: usize) -> TypeId {
        assert!(idx < self.members_count, "type member index out of bounds");
        TypeId::from(read_u16_le(self.members_bytes, idx * TypeMember::SIZE + 2))
    }

    pub fn get_name(&self, idx: usize) -> TypeNameEntry {
        assert!(idx < self.names_count, "type name index out of bounds");
        let offset = idx * TypeNameEntry::SIZE;
        TypeNameEntry::new(
            StringId::try_from(read_u16_le(self.names_bytes, offset))
                .expect("type name string id must be non-zero"),
            TypeId::from(read_u16_le(self.names_bytes, offset + 2)),
        )
    }

    pub fn names(&self) -> impl ExactSizeIterator<Item = TypeNameEntry> + '_ {
        (0..self.names_count).map(|idx| self.get_name(idx))
    }

    /// A type name's target `type_id` without building the (`NonZero`) name
    /// `StringId` — same boundary reason as [`member_type_id`](Self::member_type_id).
    pub(crate) fn name_type_id(&self, idx: usize) -> TypeId {
        assert!(idx < self.names_count, "type name index out of bounds");
        TypeId::from(read_u16_le(self.names_bytes, idx * TypeNameEntry::SIZE + 2))
    }

    /// Number of type definitions.
    pub fn defs_count(&self) -> usize {
        self.defs_count
    }

    /// Number of type members.
    pub fn members_count(&self) -> usize {
        self.members_count
    }

    /// Number of type names.
    pub fn names_count(&self) -> usize {
        self.names_count
    }

    /// Iterate over members of a record or variant type.
    pub fn members_of(&self, def: &TypeDef) -> impl Iterator<Item = TypeMember> + '_ {
        let (start, count) = match def.decode() {
            TypeDefKind::Record {
                member_start,
                member_count,
            }
            | TypeDefKind::Variant {
                member_start,
                member_count,
            } => (member_start as usize, member_count as usize),
            _ => (0, 0),
        };
        (0..count).map(move |i| self.get_member(start + i))
    }

    /// Return the inner type when `type_id` names an Option.
    pub fn option_inner(&self, type_id: TypeId) -> Option<TypeId> {
        let type_def = self.get(type_id)?;
        match type_def.decode() {
            TypeDefKind::Wrapper {
                kind: TypeKind::Option,
                inner,
            } => Some(inner),
            _ => None,
        }
    }
}

pub struct EntryPointsView<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> EntryPointsView<'a> {
    pub fn get(&self, idx: usize) -> EntryPoint {
        assert!(idx < self.count, "entrypoint index out of bounds");
        let offset = idx * EntryPoint::SIZE;
        EntryPoint::from_bytes(&self.bytes[offset..])
    }

    /// Number of entrypoints.
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = EntryPoint> + '_ {
        (0..self.count).map(|idx| self.get(idx))
    }

    /// Find an entrypoint by name (requires StringsView for comparison).
    pub fn find_by_name(&self, name: &str, strings: &StringsView<'_>) -> Option<EntryPoint> {
        self.iter().find(|e| strings.get(e.name()) == name)
    }
}

#[cfg(test)]
mod decoded_tests;
#[cfg(test)]
mod validate_tests;
