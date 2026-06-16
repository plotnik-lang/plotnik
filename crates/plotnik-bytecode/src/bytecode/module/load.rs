//! Load-time structural validation — the trust boundary.
//!
//! Everything here runs inside [`Module::load`]. It turns untrusted bytes into a
//! verified [`Module`]; once these checks pass, the rest of the crate trusts the
//! module completely (the no-panic guarantee, see `effect_stack.rs`). On any
//! malformed input it returns a [`ModuleError`], never panics.

use std::io;

use super::super::effects::{EffectOp, EffectOpcode};
use super::super::nav::Nav;
use super::super::node_type_ir::NodeTypeIR;
use super::super::{SECTION_ALIGN, VERSION};
use super::*;

/// Module load error.
///
/// Every variant is raised at the trust boundary (this module and
/// `effect_stack.rs`); the reader side never constructs one. Re-exported as
/// `super::ModuleError` for the rest of the crate.
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
    #[error("invalid type name at index {0}")]
    InvalidTypeName(usize),
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

/// Round `value` up to the next multiple of `align` in `u64` (overflow-free).
///
/// The `u64` width lets [`Module::validate_section_bounds`] re-derive the section
/// layout from a possibly-corrupt header without the overflow the real `u32`
/// [`Header::compute_offsets`] would hit.
#[inline]
fn align_up_u64(value: u64, align: u64) -> u64 {
    (value + align - 1) & !(align - 1)
}

impl Module {
    /// Load a module from storage.
    pub(super) fn from_storage(storage: ByteStorage) -> Result<Self, ModuleError> {
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

        let mut module = Self {
            storage,
            header,
            offsets,
            regex_dfas: RegexDfas::default(),
        };
        // Validation deserializes every regex DFA to prove it well-formed; it
        // hands those owned automata back so the VM reuses them instead of
        // re-deserializing per evaluation (issue #426).
        module.regex_dfas = module.validate()?;
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
    fn validate(&self) -> Result<RegexDfas, ModuleError> {
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
        let regex_dfas = self.load_regex_dfas()?;
        self.validate_type_defs()?;
        self.validate_type_names()?;
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
        Ok(regex_dfas)
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

    /// Deserialize every regex DFA once, validating each and caching the owned
    /// automaton for the VM to reuse on every evaluation (issue #426).
    ///
    /// [`deserialize_dfa`](crate::deserialize_dfa) (`DFA::from_bytes`) validates
    /// the whole serialized automaton — rejecting empty or corrupt bytes — so
    /// this single pass *is* the validation: a deserializable DFA for every real
    /// entry is the invariant the VM's hot predicate path relies on, after which
    /// `RegexDfas::is_match` searches the cached automaton with no further
    /// deserialization. The owned copy detaches the DFA from `self.storage`, so
    /// the cache lives in `Module` without a self-referential borrow. Index 0 is
    /// the reserved sentinel — never evaluated — so it stays `None` and the scan
    /// starts at 1. The offset table is already bounded by
    /// [`Self::validate_regex_table`], so `get_by_index` here cannot slice out of
    /// range.
    fn load_regex_dfas(&self) -> Result<RegexDfas, ModuleError> {
        let regexes = self.regexes();
        let count = self.header.regex_table_count as usize;
        let mut dfas = Vec::with_capacity(count);
        dfas.push(None); // index 0: reserved sentinel, never evaluated
        for i in 1..count {
            let bytes = regexes.get_by_index(i);
            let dfa = crate::deserialize_dfa(bytes)
                .map_err(|_| ModuleError::InvalidRegexDfa(i))?
                .to_owned();
            dfas.push(Some(dfa));
        }
        Ok(RegexDfas::new(dfas))
    }

    /// Validate every TypeDef: a known kind, member runs that stay inside the
    /// TypeMembers section, and every referenced TypeId — a wrapper/alias inner
    /// type or a struct/enum member type — addressing a real def, so the
    /// materializer never resolves a type out of range
    /// (`docs/binary-format/04-types.md`).
    fn validate_type_defs(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let members = self.header.type_members_count as u32;
        let type_defs = self.header.type_defs_count;

        for i in 0..types.defs_count() {
            let invalid = || ModuleError::InvalidTypeDef(i);
            let def = types.get_def(i);
            // Reject an unknown kind here, so the typed reads below cannot panic.
            let Some(data) = def.try_classify() else {
                return Err(invalid());
            };
            // Fields the kind does not name are reserved-zero
            // (docs/binary-format/04-types.md); smuggled state there must not load.
            let (raw_data, raw_count) = def.member_range();
            match data {
                TypeData::Primitive(_) => {
                    if raw_data != 0 || raw_count != 0 {
                        return Err(invalid());
                    }
                }
                TypeData::Wrapper { inner, .. } => {
                    if raw_count != 0 || inner.0 >= type_defs {
                        return Err(invalid());
                    }
                }
                TypeData::Composite {
                    member_start,
                    member_count,
                    ..
                } => {
                    // Both fields carry meaning here; only the run bound applies.
                    if member_start as u32 + member_count as u32 > members {
                        return Err(invalid());
                    }
                    let start = member_start as usize;
                    if (start..start + member_count as usize)
                        .any(|m| types.member_type_id(m).0 >= type_defs)
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(())
    }

    /// Every TypeName must target a real TypeDef; its name `StringId` is checked
    /// separately by [`validate_string_ids`](Self::validate_string_ids).
    fn validate_type_names(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let type_defs = self.header.type_defs_count;
        for i in 0..types.names_count() {
            if types.name_type_id(i).0 >= type_defs {
                return Err(ModuleError::InvalidTypeName(i));
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
            // `load_regex_dfas` leaves empty, so its DFA slot is `None`, and the
            // VM `.expect()`s a populated slot. A string operand of 0 is benign —
            // the validated easter-egg entry, never
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
}
