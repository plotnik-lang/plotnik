//! Bytecode module with unified storage.
//!
//! The [`Module`] struct holds compiled bytecode, decoding instructions lazily
//! when the VM steps into them.

use std::io;
use std::ops::Deref;
use std::path::Path;

use super::aligned_vec::AlignedVec;
use super::effects::{EffectOp, EffectOpcode};
use super::header::{Header, SectionOffsets};
use super::ids::{StringId, TypeId};
use super::instructions::{Call, Match, Opcode, Return, Trampoline};
use super::nav::Nav;
use super::node_type_ir::NodeTypeIR;
use super::sections::{FieldSymbol, NodeSymbol};
use super::type_meta::{TypeData, TypeDef, TypeKind, TypeMember, TypeName};
use super::{Entrypoint, SECTION_ALIGN, STEP_SIZE, VERSION};

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

/// Round `value` up to the next multiple of `align` in `u64` (overflow-free).
#[inline]
fn align_up_u64(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

/// Storage for bytecode bytes with guaranteed 64-byte alignment.
///
/// All bytecode must be 64-byte aligned for DFA deserialization and cache
/// efficiency. This enum ensures alignment through two paths:
/// - `Static`: Pre-aligned via `include_query_aligned!` macro
/// - `Aligned`: Allocated with 64-byte alignment via `AlignedVec`
pub enum ByteStorage {
    /// Static bytes from `include_query_aligned!` (zero-copy, pre-aligned).
    Static(&'static [u8]),
    /// Owned bytes with guaranteed 64-byte alignment.
    Aligned(AlignedVec),
}

impl Deref for ByteStorage {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            ByteStorage::Static(s) => s,
            ByteStorage::Aligned(v) => v,
        }
    }
}

impl std::fmt::Debug for ByteStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteStorage::Static(s) => f.debug_tuple("Static").field(&s.len()).finish(),
            ByteStorage::Aligned(v) => f.debug_tuple("Aligned").field(&v.len()).finish(),
        }
    }
}

impl ByteStorage {
    /// Create from static bytes (zero-copy).
    ///
    /// The bytes must be 64-byte aligned. Use `include_query_aligned!` macro.
    ///
    /// # Panics
    /// Panics if bytes are not 64-byte aligned.
    pub fn from_static(bytes: &'static [u8]) -> Self {
        assert!(
            (bytes.as_ptr() as usize).is_multiple_of(64),
            "static bytes must be 64-byte aligned; use include_query_aligned! macro"
        );
        Self::Static(bytes)
    }

    /// Create from an aligned vector (from compiler or file read).
    pub fn from_aligned(vec: AlignedVec) -> Self {
        Self::Aligned(vec)
    }

    /// Create by copying bytes into aligned storage.
    ///
    /// Use this when receiving bytes from unknown sources (e.g., network).
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        Self::Aligned(AlignedVec::copy_from_slice(bytes))
    }

    /// Read a file into aligned storage.
    pub fn from_file(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self::Aligned(AlignedVec::from_file(path)?))
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

        let opcode = Opcode::from_u8(bytes[0] & 0xF).expect("invalid opcode");
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
    #[error("malformed header: reserved bytes must be zero")]
    MalformedHeader,
    #[error("section out of bounds: header counts exceed the {total}-byte file")]
    SectionOutOfBounds { total: u32 },
    #[error("checksum mismatch: header {expected:#010x}, computed {actual:#010x}")]
    ChecksumMismatch { expected: u32, actual: u32 },
    #[error("malformed string table")]
    MalformedStringTable,
    #[error("malformed regex table")]
    MalformedRegexTable,
    #[error("invalid regex DFA at index {0}")]
    InvalidRegexDfa(usize),
    #[error("invalid type definition at index {0}")]
    InvalidTypeDef(usize),
    #[error("invalid entrypoint at index {0}")]
    InvalidEntrypoint(usize),
    #[error("invalid opcode {opcode:#x} at step {step}")]
    InvalidOpcode { step: u16, opcode: u8 },
    #[error("string id out of range at index {0}")]
    InvalidStringId(usize),
    #[error("predicate operand out of range at step {0}")]
    InvalidPredicateOperand(usize),
    #[error("malformed transitions section")]
    MalformedTransitions,
    #[error("effect stack imbalance at step {0}")]
    EffectStackImbalance(u16),
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
    /// Load a module from an aligned vector (compiler output).
    ///
    /// This is the primary constructor for bytecode produced by the compiler.
    pub fn from_aligned(vec: AlignedVec) -> Result<Self, ModuleError> {
        Self::from_storage(ByteStorage::from_aligned(vec))
    }

    /// Load a module from static bytes (zero-copy).
    ///
    /// Use with `include_query_aligned!` to embed aligned bytecode:
    /// ```ignore
    /// use plotnik_lib::include_query_aligned;
    ///
    /// let module = Module::from_static(include_query_aligned!("query.ptk.bin"))?;
    /// ```
    ///
    /// # Panics
    /// Panics if bytes are not 64-byte aligned.
    pub fn from_static(bytes: &'static [u8]) -> Result<Self, ModuleError> {
        Self::from_storage(ByteStorage::from_static(bytes))
    }

    /// Load a module from a file path.
    ///
    /// Reads the file into 64-byte aligned storage.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, ModuleError> {
        Self::from_storage(ByteStorage::from_file(&path)?)
    }

    /// Load a module from arbitrary bytes (copies into aligned storage).
    ///
    /// Use this for bytes from unknown sources (network, etc.). Always copies.
    pub fn load(bytes: &[u8]) -> Result<Self, ModuleError> {
        Self::from_storage(ByteStorage::copy_from_slice(bytes))
    }

    /// Load a module from owned bytes (copies into aligned storage).
    #[deprecated(
        since = "0.1.0",
        note = "use `Module::from_aligned` for AlignedVec or `Module::load` for copying"
    )]
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, ModuleError> {
        Self::load(&bytes)
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

        // Bound every section against the file in u64 *before* `compute_offsets`
        // does its u32 arithmetic: a corrupt header with a near-`u32::MAX` blob
        // size or count would otherwise overflow that arithmetic and panic
        // (debug) instead of returning an error.
        Self::validate_section_bounds(&header)?;

        // Section bounds held, so the u32 offset arithmetic cannot wrap.
        let offsets = header.compute_offsets();

        let module = Self {
            storage,
            header,
            offsets,
        };
        module.validate()?;
        Ok(module)
    }

    /// Validate a loaded module so later *view* accesses cannot panic and
    /// accidental corruption of the body is detected.
    ///
    /// Section bounds are checked earlier, in [`validate_section_bounds`], before
    /// offsets are computed. The remaining checks here defend a corrupted header
    /// (which the CRC does not cover) whose counts/sizes would otherwise drive
    /// out-of-bounds slicing: the reserved bytes are zero, the CRC32 over the
    /// post-header body matches, the string/regex table sentinels are
    /// well-formed, the documented TypeDef member ranges stay in bounds, and
    /// entrypoint targets address real steps.
    ///
    /// The CRC32 detects *accidental* corruption of the body — the format's
    /// threat model (truncation, bit-rot). It is not a MAC, so a deliberately
    /// forged module can recompute a matching checksum over crafted bytes;
    /// [`Self::validate_transitions`] therefore re-verifies the lazily-decoded
    /// instruction stream structurally, and
    /// [`validate_effect_stack`](super::effect_stack::validate_effect_stack)
    /// proves no path can panic the materializer's builder stack or the VM's
    /// suppression counter — so a loaded module never panics on view/decode/VM
    /// access regardless of how it was crafted.
    fn validate(&self) -> Result<(), ModuleError> {
        // Reserved header bytes are not covered by the CRC; v5 fixes them at zero.
        if self.header._reserved != [0u8; 22] {
            return Err(ModuleError::MalformedHeader);
        }

        let computed = crc32fast::hash(&self.storage[64..]);
        if computed != self.header.checksum {
            return Err(ModuleError::ChecksumMismatch {
                expected: self.header.checksum,
                actual: computed,
            });
        }

        self.validate_string_table()?;
        self.validate_regex_table()?;
        self.validate_regex_dfas()?;
        self.validate_type_defs()?;
        // Bound every embedded `StringId` before any later check constructs a
        // (`NonZero`) `StringId` from one — e.g. `validate_entrypoints` builds an
        // `Entrypoint`, which would otherwise panic on a forged zero name.
        self.validate_string_ids()?;
        let is_start = self.validate_transitions()?;
        self.validate_entrypoints(&is_start)?;
        // Structural validity (every step decodes, every jump lands on a start)
        // is now established, so the effect-stack walk can use the safe typed
        // instruction API. This closes the last forged-module panic class: the
        // materializer's builder-stack panics and the VM's suppression underflow.
        super::effect_stack::validate_effect_stack(self)?;
        Ok(())
    }

    /// Recompute the section layout in `u64` (no overflow) and ensure every
    /// section, up to and including Transitions, fits inside the file.
    ///
    /// Runs on the raw header *before* [`Header::compute_offsets`], so a corrupt
    /// header cannot drive that u32 arithmetic to overflow. Sections are laid
    /// out consecutively with alignment padding, so verifying the final
    /// Transitions end also bounds every earlier section. Passing this check
    /// also proves the `u32` [`SectionOffsets`] will not wrap, so the view
    /// methods can trust them.
    fn validate_section_bounds(h: &Header) -> Result<(), ModuleError> {
        let total = h.total_size;
        let align = SECTION_ALIGN as u64;
        let oob = || ModuleError::SectionOutOfBounds { total };

        let mut cursor = align; // str_blob starts right after the header
        cursor = align_up_u64(cursor + h.str_blob_size as u64, align);
        cursor = align_up_u64(cursor + h.regex_blob_size as u64, align);
        cursor = align_up_u64(cursor + (h.str_table_count as u64 + 1) * 4, align);
        cursor = align_up_u64(cursor + (h.regex_table_count as u64 + 1) * 8, align);
        cursor = align_up_u64(cursor + h.node_types_count as u64 * 4, align);
        cursor = align_up_u64(cursor + h.node_fields_count as u64 * 4, align);
        cursor = align_up_u64(cursor + h.type_defs_count as u64 * 4, align);
        cursor = align_up_u64(cursor + h.type_members_count as u64 * 4, align);
        cursor = align_up_u64(cursor + h.type_names_count as u64 * 4, align);
        cursor = align_up_u64(cursor + h.entrypoints_count as u64 * 8, align);
        // `cursor` now points at the Transitions section.
        let transitions_end = cursor + h.transitions_count as u64 * STEP_SIZE as u64;

        if transitions_end > total as u64 {
            return Err(oob());
        }
        Ok(())
    }

    /// The string offset table is `count + 1` ascending `u32` offsets ending at
    /// the blob length; each delimited slice must be valid UTF-8.
    fn validate_string_table(&self) -> Result<(), ModuleError> {
        let table = self.string_table_slice();
        let blob_len = self.header.str_blob_size;
        let count = self.header.str_table_count as usize;
        let blob = &self.storage[self.offsets.str_blob as usize..][..blob_len as usize];

        let mut prev = 0u32;
        for i in 0..=count {
            let off = read_u32_le(table, i * 4);
            if off < prev || off > blob_len {
                return Err(ModuleError::MalformedStringTable);
            }
            if i > 0 && std::str::from_utf8(&blob[prev as usize..off as usize]).is_err() {
                return Err(ModuleError::MalformedStringTable);
            }
            prev = off;
        }
        if prev != blob_len {
            return Err(ModuleError::MalformedStringTable);
        }
        Ok(())
    }

    /// The regex table is `count + 1` entries whose DFA offsets ascend and end
    /// at the blob length, so [`RegexView::get_by_index`] never slices OOB.
    fn validate_regex_table(&self) -> Result<(), ModuleError> {
        let table = self.regex_table_slice();
        let blob_len = self.header.regex_blob_size;
        let count = self.header.regex_table_count as usize;

        let mut prev = 0u32;
        for i in 0..=count {
            // Entry layout: string_id (u16) | reserved (u16) | offset (u32).
            let off = read_u32_le(table, i * 8 + 4);
            if off < prev || off > blob_len {
                return Err(ModuleError::MalformedRegexTable);
            }
            prev = off;
        }
        if prev != blob_len {
            return Err(ModuleError::MalformedRegexTable);
        }
        Ok(())
    }

    /// Every regex entry's serialized sparse DFA must deserialize, so the VM's
    /// per-evaluation [`deserialize_dfa`](crate::deserialize_dfa) (which the hot
    /// predicate path `.expect()`s) and the `!is_empty()` assertion are sound
    /// invariants, not reachable panics on a forged blob. Index 0 is the reserved
    /// sentinel — never evaluated — so the scan starts at 1. The offset table is
    /// already bounded by [`Self::validate_regex_table`], so `get_by_index` here
    /// cannot slice out of range.
    fn validate_regex_dfas(&self) -> Result<(), ModuleError> {
        let regexes = self.regexes();
        for i in 1..self.header.regex_table_count as usize {
            let bytes = regexes.get_by_index(i);
            if bytes.is_empty() || crate::deserialize_dfa(bytes).is_err() {
                return Err(ModuleError::InvalidRegexDfa(i));
            }
        }
        Ok(())
    }

    /// Every TypeDef must have a known kind, and Struct/Enum member ranges must
    /// stay inside the TypeMembers section (`docs/binary-format/04-types.md`).
    fn validate_type_defs(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let members = self.header.type_members_count as u32;
        for i in 0..types.defs_count() {
            let def = types.get_def(i);
            let Some(kind) = TypeKind::from_u8(def.kind_byte()) else {
                return Err(ModuleError::InvalidTypeDef(i));
            };
            if kind.is_composite() {
                let (start, count) = def.member_range();
                if start as u32 + count as u32 > members {
                    return Err(ModuleError::InvalidTypeDef(i));
                }
            }
        }
        Ok(())
    }

    /// Entrypoint targets must address a real step so the VM's first
    /// [`decode_step`](Self::decode_step) cannot read out of bounds.
    /// `is_start` is the instruction-start bitmap from
    /// [`Self::validate_transitions`]: a `target` that lands inside a multi-step
    /// instruction would make the VM start decoding mid-instruction, so it must
    /// be an instruction start, not merely in range.
    fn validate_entrypoints(&self, is_start: &[bool]) -> Result<(), ModuleError> {
        let entrypoints = self.entrypoints();
        let steps = self.header.transitions_count;
        let type_defs = self.header.type_defs_count;
        for i in 0..entrypoints.len() {
            let ep = entrypoints.get(i);
            let target = ep.target();
            if target >= steps || !is_start[target as usize] || ep.result_type().0 >= type_defs {
                return Err(ModuleError::InvalidEntrypoint(i));
            }
        }
        Ok(())
    }

    /// Every *required* `StringId` held in a section — entrypoint names,
    /// node/field symbol names, type names, type member names, and regex pattern
    /// names — must address a real string-table entry, so the view accessors that
    /// resolve them (and `find_by_name`, the materializer's struct-field keys,
    /// etc.) never slice out of bounds. The table holds `str_table_count + 1`
    /// offsets, so the valid id range is `0..str_table_count`. This upholds the
    /// format's guarantee that a loaded module never panics on view access
    /// (`docs/binary-format/01-overview.md`).
    fn validate_string_ids(&self) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let n = self.header.str_table_count;

        // Read the raw `u16` rather than the typed accessor: a required `StringId`
        // is a `NonZeroU16`, so `StringId::new(0)` on a forged zero would panic
        // here in the validator itself, defeating the purpose. A valid required id
        // is a real, non-easter-egg entry: `1..str_table_count`. Section bounds are
        // already proven by `validate_section_bounds`, so the reads stay in range.
        let check = |base: u32, stride: usize, name_off: usize, start: usize, count: usize| {
            let base = base as usize;
            for i in start..count {
                let raw = read_u16_le(storage, base + i * stride + name_off);
                if raw == 0 || raw >= n {
                    return Err(ModuleError::InvalidStringId(i));
                }
            }
            Ok(())
        };

        // entrypoint name: u16 at entry+0 (8-byte entries)
        check(
            self.offsets.entrypoints,
            8,
            0,
            0,
            self.header.entrypoints_count as usize,
        )?;
        // node/field symbol name: u16 at entry+2 (4-byte entries)
        check(
            self.offsets.node_types,
            4,
            2,
            0,
            self.header.node_types_count as usize,
        )?;
        check(
            self.offsets.node_fields,
            4,
            2,
            0,
            self.header.node_fields_count as usize,
        )?;
        // type name / member name: u16 at entry+0 (4-byte entries)
        check(
            self.offsets.type_names,
            4,
            0,
            0,
            self.header.type_names_count as usize,
        )?;
        check(
            self.offsets.type_members,
            4,
            0,
            0,
            self.header.type_members_count as usize,
        )?;
        // regex pattern name: u16 at entry+0 (8-byte entries). Index 0 is the
        // reserved sentinel — never resolved — so start at 1; `dump`/`trace`
        // resolve `string_id` for every real entry through the panicking
        // `RegexView::get_string_id` (and then index the string blob).
        check(
            self.offsets.regex_table,
            8,
            0,
            1,
            self.header.regex_table_count as usize,
        )?;
        Ok(())
    }

    /// Structurally re-verify the whole instruction stream so the documented
    /// guarantee — a loaded module never panics on view/decode access — holds
    /// for *any* module whose header and CRC check out, including a deliberately
    /// forged one.
    ///
    /// A module is decoded lazily: [`decode_step`](Self::decode_step) and the
    /// per-opcode decoders, the effect/predicate iterators, and the materializer
    /// all build `NonZero`/enum values and index tables straight from
    /// instruction bytes. Each is a panic site on crafted input — `Opcode`,
    /// `Nav`, `NodeTypeIR`, `EffectOpcode`, and `StepId::new` decoding, plus
    /// `get_member` / `get_by_index` table lookups. This walk rejects every such
    /// input up front, reading only through checked slicing so it never panics
    /// itself.
    ///
    /// Two passes over the stream:
    /// 1. Decode each instruction's fixed-size slot (the slot size is fixed by
    ///    the opcode, so the walk is unambiguous), validating opcode, segment,
    ///    nav, node kind, effect opcodes, `Set`/`Enum` member operands, and
    ///    predicate operands, and rejecting any zero successor address. Record
    ///    each instruction start and collect every jump target.
    /// 2. Every collected jump target — successor, call next/target, trampoline
    ///    next — must land on a recorded instruction start.
    ///
    /// Returns the instruction-start bitmap so [`Self::validate_entrypoints`] can
    /// hold entrypoint targets to the same rule: an entrypoint pointing into the
    /// interior of a multi-step instruction would otherwise begin decoding
    /// mid-instruction.
    ///
    /// Out of scope (not a decode/view panic): node-kind/field ids, which are
    /// resolved against the tree-sitter grammar at match time, and member
    /// `type_id`s, which the materializer reads through the checked `Types::get`
    /// that returns `Option`.
    fn validate_transitions(&self) -> Result<Vec<bool>, ModuleError> {
        let storage: &[u8] = &self.storage;
        let base = self.offsets.transitions as usize;
        let steps = self.header.transitions_count;

        let read_u8 = |off: usize| {
            storage
                .get(off)
                .copied()
                .ok_or(ModuleError::MalformedTransitions)
        };
        let read_u16 = |off: usize| {
            storage
                .get(off..off + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or(ModuleError::MalformedTransitions)
        };

        let mut is_start = vec![false; steps as usize];
        let mut targets: Vec<u16> = Vec::new();

        let mut step: u16 = 0;
        while step < steps {
            is_start[step as usize] = true;
            let instr_off = base + step as usize * STEP_SIZE;
            let header = read_u8(instr_off)?;

            let nibble = header & 0x0F;
            let Some(opcode) = Opcode::from_u8(nibble) else {
                return Err(ModuleError::InvalidOpcode {
                    step,
                    opcode: nibble,
                });
            };
            // Every opcode reserves the segment bits; the call/return/trampoline
            // decoders `assert!` segment == 0, and a non-zero segment is unused.
            if (header >> 6) & 0x3 != 0 {
                return Err(ModuleError::MalformedTransitions);
            }

            match opcode {
                Opcode::Return => {}
                Opcode::Trampoline => {
                    let next = read_u16(instr_off + 2)?;
                    if next == 0 {
                        return Err(ModuleError::MalformedTransitions);
                    }
                    targets.push(next);
                }
                Opcode::Call => {
                    // `Call::from_bytes` decodes a nav and two non-zero `StepId`s.
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedTransitions);
                    }
                    let next = read_u16(instr_off + 4)?;
                    let target = read_u16(instr_off + 6)?;
                    if next == 0 || target == 0 {
                        return Err(ModuleError::MalformedTransitions);
                    }
                    targets.push(next);
                    targets.push(target);
                }
                _ => {
                    // A Match variant (`Match8` or extended).
                    let node_kind = (header >> 4) & 0x3;
                    if NodeTypeIR::try_from_bytes(node_kind, read_u16(instr_off + 2)?).is_none() {
                        return Err(ModuleError::MalformedTransitions);
                    }
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedTransitions);
                    }

                    if opcode == Opcode::Match8 {
                        // bytes 6-7 hold the single successor; `0` means terminal.
                        let next = read_u16(instr_off + 6)?;
                        if next != 0 {
                            targets.push(next);
                        }
                    } else {
                        self.validate_extended_match(opcode, instr_off, step, &mut targets)?;
                    }
                }
            }

            step = step
                .checked_add(opcode.step_count())
                .ok_or(ModuleError::MalformedTransitions)?;
        }

        // A well-formed stream tiles the section in whole instructions. An
        // overrun means a trailing instruction's slot crosses the section end,
        // so a successor pointing into it could later decode past the buffer.
        if step != steps {
            return Err(ModuleError::MalformedTransitions);
        }

        for t in targets {
            if t >= steps || !is_start[t as usize] {
                return Err(ModuleError::MalformedTransitions);
            }
        }
        Ok(is_start)
    }

    /// Validate the payload of one extended `Match` (`Match16`..`Match64`):
    /// effects, predicate, and successors. Appends each successor to `targets`
    /// for the pass-2 jump-target check in [`Self::validate_transitions`].
    fn validate_extended_match(
        &self,
        opcode: Opcode,
        instr_off: usize,
        step: u16,
        targets: &mut Vec<u16>,
    ) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let read_u16 = |off: usize| {
            storage
                .get(off..off + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .ok_or(ModuleError::MalformedTransitions)
        };

        let counts = read_u16(instr_off + 6)?;
        let pre = ((counts >> 13) & 0x7) as usize;
        let neg = ((counts >> 10) & 0x7) as usize;
        let post = ((counts >> 7) & 0x7) as usize;
        let succ = ((counts >> 2) & 0x1F) as usize;
        let has_predicate = (counts >> 1) & 0x1 != 0;

        // Every payload slot the decoders read — effects, predicate, successors —
        // must lie within this instruction's fixed-size slot, or the iterators
        // read into the next instruction (or past the buffer at the stream end).
        let used = pre + neg + post + if has_predicate { 2 } else { 0 } + succ;
        if used > opcode.payload_slots() {
            return Err(ModuleError::MalformedTransitions);
        }

        // Pre/post effect opcodes are decoded (neg fields are plain `u16`); a
        // `Set`/`Enum` operand indexes the type-member table via the
        // materializer's `get_member`, which asserts the index is in bounds.
        let members = self.header.type_members_count;
        let check_effect = |slot: usize| -> Result<(), ModuleError> {
            let off = instr_off + 8 + slot * 2;
            let b = storage
                .get(off..off + 2)
                .ok_or(ModuleError::MalformedTransitions)?;
            let op =
                EffectOp::try_from_bytes([b[0], b[1]]).ok_or(ModuleError::MalformedTransitions)?;
            if matches!(op.opcode, EffectOpcode::Set | EffectOpcode::Enum)
                && op.payload as u16 >= members
            {
                return Err(ModuleError::MalformedTransitions);
            }
            Ok(())
        };
        for i in 0..pre {
            check_effect(i)?;
        }
        for i in 0..post {
            check_effect(pre + neg + i)?;
        }

        if has_predicate {
            let pred_off = instr_off + 8 + (pre + neg + post) * 2;
            let b = storage
                .get(pred_off..pred_off + 4)
                .ok_or(ModuleError::MalformedTransitions)?;
            let op_and_flags = u16::from_le_bytes([b[0], b[1]]);
            let op = (op_and_flags & 0xFF) as u8;
            let is_regex = (op_and_flags >> 8) & 0x1 != 0;
            let value_ref = u16::from_le_bytes([b[2], b[3]]);

            // The operator must be a known predicate op (0..=6), the regex flag
            // must agree with the operator's class, and the operand must index
            // its table — otherwise `PredicateOp::from_byte`, `get_by_index`, or
            // the VM's op/flag `unreachable!` would panic when this predicate is
            // evaluated or dumped. The regex operand must be a *real* entry
            // (`1..count`): index 0 is the reserved sentinel that
            // `validate_regex_dfas` skips, so its DFA bytes are unvalidated and
            // empty, and the VM `.expect()`s non-empty regex bytes. A string
            // operand of 0 is benign — the validated easter-egg entry, never
            // asserted non-empty.
            let op_is_regex = matches!(op, 5 | 6); // RegexMatch | RegexNoMatch
            let operand_ok = if is_regex {
                (1..self.header.regex_table_count).contains(&value_ref)
            } else {
                value_ref < self.header.str_table_count
            };
            if op > 6 || op_is_regex != is_regex || !operand_ok {
                return Err(ModuleError::InvalidPredicateOperand(step as usize));
            }
        }

        let succ_off = instr_off + 8 + (pre + neg + post) * 2 + if has_predicate { 4 } else { 0 };
        for i in 0..succ {
            let next = read_u16(succ_off + i * 2)?;
            // An extended successor decodes through `StepId::new`, which panics
            // on zero; `0` is the terminal marker only for the `Match8` slot.
            if next == 0 {
                return Err(ModuleError::MalformedTransitions);
            }
            targets.push(next);
        }
        Ok(())
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
        &self.storage[offset..offset + (count + 1) * 8]
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

/// View into the regex table for lazy DFA lookup.
///
/// Table format per entry: `string_id (u16) | reserved (u16) | offset (u32)` = 8 bytes.
/// This allows access to both the pattern string (via StringTable) and DFA bytes.
pub struct RegexView<'a> {
    blob: &'a [u8],
    table: &'a [u8],
}

impl<'a> RegexView<'a> {
    /// Entry size in bytes: string_id (u16) + reserved (u16) + offset (u32).
    const ENTRY_SIZE: usize = 8;

    /// Get regex DFA bytes by index.
    ///
    /// Returns the raw DFA bytes for the regex at the given index.
    /// Use `regex-automata` to deserialize: `DFA::from_bytes(&bytes)`.
    pub fn get_by_index(&self, idx: usize) -> &'a [u8] {
        let entry_offset = idx * Self::ENTRY_SIZE;
        let next_entry_offset = (idx + 1) * Self::ENTRY_SIZE;

        let start = read_u32_le(self.table, entry_offset + 4) as usize;
        let end = read_u32_le(self.table, next_entry_offset + 4) as usize;
        &self.blob[start..end]
    }

    /// Get the StringId of the pattern for a regex by index.
    ///
    /// This allows looking up the pattern text from StringTable for display.
    pub fn get_string_id(&self, idx: usize) -> super::StringId {
        let entry_offset = idx * Self::ENTRY_SIZE;
        let string_id = read_u16_le(self.table, entry_offset);
        super::StringId::new(string_id)
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
