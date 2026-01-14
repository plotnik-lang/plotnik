//! Bytecode module with unified storage.
//!
//! The [`Module`] struct holds compiled bytecode, decoding instructions lazily
//! when the VM steps into them.

use std::io;
use std::ops::Deref;
use std::path::Path;

use super::header::{Header, SectionOffsets};
use super::ids::{StringId, TypeId};
use super::instructions::{Call, Match, Opcode, Return, Trampoline};
use super::sections::{FieldSymbol, NodeSymbol, TriviaEntry};
use super::type_meta::{TypeData, TypeDef, TypeKind, TypeMember, TypeName};
use super::{Entrypoint, STEP_SIZE, VERSION};

/// Read a little-endian u16 from bytes at the given offset.
#[inline]
fn read_u16_le(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

/// Read a little-endian u32 from bytes at the given offset.
#[inline]
fn read_u32_le(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

/// Storage for bytecode bytes.
#[derive(Debug)]
pub struct ByteStorage(Vec<u8>);

impl Deref for ByteStorage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ByteStorage {
    /// Create from owned bytes.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    /// Read a file into memory.
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Ok(Self(bytes))
    }
}

/// Decoded instruction from bytecode.
#[derive(Clone, Copy, Debug)]
pub enum Instruction<'a> {
    Match(Match<'a>),
    Call(Call),
    Return(Return),
    Trampoline(Trampoline),
}

impl<'a> Instruction<'a> {
    /// Decode an instruction from bytecode bytes.
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        debug_assert!(bytes.len() >= 8, "instruction too short");

        let opcode = Opcode::from_u8(bytes[0] & 0xF);
        match opcode {
            Opcode::Call => {
                let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                Self::Call(Call::from_bytes(arr))
            }
            Opcode::Return => {
                let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                Self::Return(Return::from_bytes(arr))
            }
            Opcode::Trampoline => {
                let arr: [u8; 8] = bytes[..8].try_into().unwrap();
                Self::Trampoline(Trampoline::from_bytes(arr))
            }
            _ => Self::Match(Match::from_bytes(bytes)),
        }
    }
}

/// Module load error.
#[derive(Debug, thiserror::Error)]
pub enum ModuleError {
    #[error("invalid magic: expected PTKQ")]
    InvalidMagic,
    #[error("unsupported version: {0} (expected {VERSION})")]
    UnsupportedVersion(u32),
    #[error("file too small: {0} bytes (minimum 64)")]
    FileTooSmall(usize),
    #[error("size mismatch: header says {header} bytes, got {actual}")]
    SizeMismatch { header: u32, actual: usize },
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// A compiled bytecode module.
///
/// Instructions are decoded lazily via [`decode_step`](Self::decode_step).
/// Cold data (strings, symbols, types) is accessed through view methods.
#[derive(Debug)]
pub struct Module {
    storage: ByteStorage,
    header: Header,
    /// Cached section offsets (computed from header counts).
    offsets: SectionOffsets,
}

impl Module {
    /// Load a module from owned bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ModuleError> {
        Self::from_storage(ByteStorage::from_vec(bytes))
    }

    /// Load a module from a file path.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ModuleError> {
        let storage = ByteStorage::from_file(&path)?;
        Self::from_storage(storage)
    }

    /// Load a module from storage.
    fn from_storage(storage: ByteStorage) -> Result<Self, ModuleError> {
        if storage.len() < 64 {
            return Err(ModuleError::FileTooSmall(storage.len()));
        }

        let header = Header::from_bytes(&storage[..64]);

        if !header.validate_magic() {
            return Err(ModuleError::InvalidMagic);
        }
        if !header.validate_version() {
            return Err(ModuleError::UnsupportedVersion(header.version));
        }
        if header.total_size as usize != storage.len() {
            return Err(ModuleError::SizeMismatch {
                header: header.total_size,
                actual: storage.len(),
            });
        }

        // Compute all section offsets from header counts and blob sizes
        let offsets = header.compute_offsets();

        Ok(Self {
            storage,
            header,
            offsets,
        })
    }

    /// Get the parsed header.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Get the computed section offsets.
    pub fn offsets(&self) -> &SectionOffsets {
        &self.offsets
    }

    /// Get the raw bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.storage
    }

    /// Decode an instruction at the given step index.
    #[inline]
    pub fn decode_step(&self, step: u16) -> Instruction<'_> {
        let offset = self.offsets.transitions as usize + (step as usize) * STEP_SIZE;
        Instruction::from_bytes(&self.storage[offset..])
    }

    /// Get a view into the string table.
    pub fn strings(&self) -> StringsView<'_> {
        StringsView {
            blob: &self.storage[self.offsets.str_blob as usize..],
            table: self.string_table_slice(),
        }
    }

    /// Get a view into the node type symbols.
    pub fn node_types(&self) -> SymbolsView<'_, NodeSymbol> {
        let offset = self.offsets.node_types as usize;
        let count = self.header.node_types_count as usize;
        SymbolsView {
            bytes: &self.storage[offset..offset + count * 4],
            count,
            _marker: std::marker::PhantomData,
        }
    }

    /// Get a view into the node field symbols.
    pub fn node_fields(&self) -> SymbolsView<'_, FieldSymbol> {
        let offset = self.offsets.node_fields as usize;
        let count = self.header.node_fields_count as usize;
        SymbolsView {
            bytes: &self.storage[offset..offset + count * 4],
            count,
            _marker: std::marker::PhantomData,
        }
    }

    /// Get a view into the trivia entries.
    pub fn trivia(&self) -> TriviaView<'_> {
        let offset = self.offsets.trivia as usize;
        let count = self.header.trivia_count as usize;
        TriviaView {
            bytes: &self.storage[offset..offset + count * 2],
            count,
        }
    }

    /// Get a view into the regex table.
    pub fn regexes(&self) -> RegexView<'_> {
        RegexView {
            blob: &self.storage[self.offsets.regex_blob as usize..],
            table: self.regex_table_slice(),
        }
    }

    /// Get a view into the type metadata.
    pub fn types(&self) -> TypesView<'_> {
        let defs_offset = self.offsets.type_defs as usize;
        let defs_count = self.header.type_defs_count as usize;
        let members_offset = self.offsets.type_members as usize;
        let members_count = self.header.type_members_count as usize;
        let names_offset = self.offsets.type_names as usize;
        let names_count = self.header.type_names_count as usize;

        TypesView {
            defs_bytes: &self.storage[defs_offset..defs_offset + defs_count * 4],
            members_bytes: &self.storage[members_offset..members_offset + members_count * 4],
            names_bytes: &self.storage[names_offset..names_offset + names_count * 4],
            defs_count,
            members_count,
            names_count,
        }
    }

    /// Get a view into the entrypoints.
    pub fn entrypoints(&self) -> EntrypointsView<'_> {
        let offset = self.offsets.entrypoints as usize;
        let count = self.header.entrypoints_count as usize;
        EntrypointsView {
            bytes: &self.storage[offset..offset + count * 8],
            count,
        }
    }

    /// Helper to get string table as bytes.
    /// The table has count+1 entries (includes sentinel for length calculation).
    fn string_table_slice(&self) -> &[u8] {
        let offset = self.offsets.str_table as usize;
        let count = self.header.str_table_count as usize;
        &self.storage[offset..offset + (count + 1) * 4]
    }

    /// Helper to get regex table as bytes.
    /// The table has count+1 entries (includes sentinel for length calculation).
    fn regex_table_slice(&self) -> &[u8] {
        let offset = self.offsets.regex_table as usize;
        let count = self.header.regex_table_count as usize;
        &self.storage[offset..offset + (count + 1) * 4]
    }
}

/// View into the string table for lazy string lookup.
pub struct StringsView<'a> {
    blob: &'a [u8],
    table: &'a [u8],
}

impl<'a> StringsView<'a> {
    /// Get a string by its ID (type-safe access for bytecode references).
    pub fn get(&self, id: StringId) -> &'a str {
        self.get_by_index(id.get() as usize)
    }

    /// Get a string by raw index (for iteration/dumps, including easter egg at 0).
    ///
    /// The string table contains sequential u32 offsets. To get string i:
    /// `start = table[i]`, `end = table[i+1]`, `length = end - start`.
    pub fn get_by_index(&self, idx: usize) -> &'a str {
        let start = read_u32_le(self.table, idx * 4) as usize;
        let end = read_u32_le(self.table, (idx + 1) * 4) as usize;
        std::str::from_utf8(&self.blob[start..end]).expect("invalid UTF-8 in string table")
    }
}

/// View into symbol tables (node types or field names).
pub struct SymbolsView<'a, T> {
    bytes: &'a [u8],
    count: usize,
    _marker: std::marker::PhantomData<T>,
}

impl<'a> SymbolsView<'a, NodeSymbol> {
    /// Get a node symbol by index.
    pub fn get(&self, idx: usize) -> NodeSymbol {
        assert!(idx < self.count, "node symbol index out of bounds");
        let offset = idx * 4;
        NodeSymbol::new(
            read_u16_le(self.bytes, offset),
            StringId::new(read_u16_le(self.bytes, offset + 2)),
        )
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<'a> SymbolsView<'a, FieldSymbol> {
    /// Get a field symbol by index.
    pub fn get(&self, idx: usize) -> FieldSymbol {
        assert!(idx < self.count, "field symbol index out of bounds");
        let offset = idx * 4;
        FieldSymbol::new(
            read_u16_le(self.bytes, offset),
            StringId::new(read_u16_le(self.bytes, offset + 2)),
        )
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// View into trivia entries.
pub struct TriviaView<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> TriviaView<'a> {
    /// Get a trivia entry by index.
    pub fn get(&self, idx: usize) -> TriviaEntry {
        assert!(idx < self.count, "trivia index out of bounds");
        TriviaEntry::new(read_u16_le(self.bytes, idx * 2))
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Check if a node type is trivia.
    pub fn contains(&self, node_type: u16) -> bool {
        (0..self.count).any(|i| self.get(i).node_type() == node_type)
    }
}

/// View into the regex table for lazy DFA lookup.
pub struct RegexView<'a> {
    blob: &'a [u8],
    table: &'a [u8],
}

impl<'a> RegexView<'a> {
    /// Get regex blob bytes by index.
    ///
    /// Returns the raw DFA bytes for the regex at the given index.
    /// Use `regex-automata` to deserialize: `DFA::from_bytes(&bytes)`.
    pub fn get_by_index(&self, idx: usize) -> &'a [u8] {
        let start = read_u32_le(self.table, idx * 4) as usize;
        let end = read_u32_le(self.table, (idx + 1) * 4) as usize;
        &self.blob[start..end]
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
    /// Get a type definition by index.
    pub fn get_def(&self, idx: usize) -> TypeDef {
        assert!(idx < self.defs_count, "type def index out of bounds");
        let offset = idx * 4;
        TypeDef::from_bytes(&self.defs_bytes[offset..])
    }

    /// Get a type definition by TypeId.
    pub fn get(&self, id: TypeId) -> Option<TypeDef> {
        let idx = id.0 as usize;
        if idx < self.defs_count {
            Some(self.get_def(idx))
        } else {
            None
        }
    }

    /// Get a type member by index.
    pub fn get_member(&self, idx: usize) -> TypeMember {
        assert!(idx < self.members_count, "type member index out of bounds");
        let offset = idx * 4;
        TypeMember::new(
            StringId::new(read_u16_le(self.members_bytes, offset)),
            TypeId(read_u16_le(self.members_bytes, offset + 2)),
        )
    }

    /// Get a type name entry by index.
    pub fn get_name(&self, idx: usize) -> TypeName {
        assert!(idx < self.names_count, "type name index out of bounds");
        let offset = idx * 4;
        TypeName::new(
            StringId::new(read_u16_le(self.names_bytes, offset)),
            TypeId(read_u16_le(self.names_bytes, offset + 2)),
        )
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

    /// Iterate over members of a struct or enum type.
    pub fn members_of(&self, def: &TypeDef) -> impl Iterator<Item = TypeMember> + '_ {
        let (start, count) = match def.classify() {
            TypeData::Composite {
                member_start,
                member_count,
                ..
            } => (member_start as usize, member_count as usize),
            _ => (0, 0),
        };
        (0..count).map(move |i| self.get_member(start + i))
    }

    /// Unwrap Optional wrapper and return (inner_type, is_optional).
    /// If not Optional, returns (type_id, false).
    pub fn unwrap_optional(&self, type_id: TypeId) -> (TypeId, bool) {
        let Some(type_def) = self.get(type_id) else {
            return (type_id, false);
        };
        match type_def.classify() {
            TypeData::Wrapper {
                kind: TypeKind::Optional,
                inner,
            } => (inner, true),
            _ => (type_id, false),
        }
    }
}

/// View into entrypoints.
pub struct EntrypointsView<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> EntrypointsView<'a> {
    /// Get an entrypoint by index.
    pub fn get(&self, idx: usize) -> Entrypoint {
        assert!(idx < self.count, "entrypoint index out of bounds");
        let offset = idx * 8;
        Entrypoint::from_bytes(&self.bytes[offset..])
    }

    /// Number of entrypoints.
    pub fn len(&self) -> usize {
        self.count
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Find an entrypoint by name (requires StringsView for comparison).
    pub fn find_by_name(&self, name: &str, strings: &StringsView<'_>) -> Option<Entrypoint> {
        (0..self.count)
            .map(|i| self.get(i))
            .find(|e| strings.get(e.name()) == name)
    }
}
