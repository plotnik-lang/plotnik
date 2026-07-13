//! Loading and structural validation of compiler-emitted bytecode.
//!
//! This loader is crate-private and accepts only compiler output. Its checks catch
//! compiler bugs before the VM trusts the bytecode. Malformed bytes
//! return [`ModuleError`]; they never become a [`Module`] or reach execution.

use super::super::effects::{Effect, EffectKind};
use super::super::instructions::{
    MATCH_PAYLOAD_START, MatchCounts, MatchPredicate, PAYLOAD_SLOT_SIZE, PREDICATE_SIZE,
    PREDICATE_SLOTS, ReturnEntry, header_byte,
};
use super::super::node_kind_constraint::NodeKindConstraint;
use super::super::sections::SymbolNameEntry;
use super::super::type_meta::{TypeDefKind, TypeMember, TypeNameEntry};
use super::super::{
    HEADER_SIZE, MAX_SPANS, SECTION_ALIGN, SPAN_ENTRY_SIZE, SPAN_NO_BINDING, SpanKind, VERSION,
};
use super::*;
use crate::bytecode::predicate_op::PredicateOp;
use plotnik_rt::Nav;
use std::collections::{HashMap, HashSet};

/// Bytecode validation error.
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
    #[error("bytecode buffer too small: {0} bytes (minimum {HEADER_SIZE})")]
    BufferTooSmall(usize),
    #[error("size mismatch: header says {header} bytes, got {actual}")]
    SizeMismatch { header: u32, actual: usize },
    #[error("malformed header: reserved bytes must be zero")]
    MalformedHeader,
    #[error("section out of bounds: header counts exceed the {total}-byte buffer")]
    SectionOutOfBounds { total: u32 },
    #[error("non-zero section padding")]
    NonZeroSectionPadding,
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
    #[error("invalid entry point at index {0}")]
    InvalidEntryPoint(usize),
    #[error("invalid opcode {opcode:#x} at instruction address {addr}")]
    InvalidOpcode { addr: CodeAddr, opcode: u8 },
    #[error("string id out of range at index {0}")]
    InvalidStringId(usize),
    #[error("invalid node symbol at index {0}")]
    InvalidNodeSymbol(usize),
    #[error("predicate operand out of range at instruction address {0}")]
    InvalidPredicateOperand(CodeAddr),
    #[error("malformed instruction stream")]
    MalformedInstructionStream,
    #[error("effect stack imbalance at instruction address {0}")]
    EffectStackImbalance(CodeAddr),
    #[error("effect stack analysis budget exceeded at instruction address {0}")]
    EffectStackBudget(CodeAddr),
    #[error("cursor depth imbalance at instruction address {0}")]
    DepthImbalance(CodeAddr),
    #[error("invalid span entry at index {0}")]
    InvalidSpanEntry(usize),
    #[error("span effect payload out of range at instruction address {0}")]
    InvalidSpanPayload(CodeAddr),
    #[error("span bracket imbalance at instruction address {0}")]
    SpanImbalance(CodeAddr),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReturnOutcomes(u8);

impl ReturnOutcomes {
    const NONE: Self = Self(0);
    const MATCHED: Self = Self(1);
    const BOTH: Self = Self(3);

    fn insert(&mut self, outcome: plotnik_rt::ReturnOutcome) {
        self.0 |= match outcome {
            plotnik_rt::ReturnOutcome::Matched => Self::MATCHED.0,
            plotnik_rt::ReturnOutcome::Empty => 2,
        };
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ReturnContract {
    outcomes: ReturnOutcomes,
    entry: Option<ReturnEntry>,
    mixed_entries: bool,
}

impl ReturnContract {
    const NONE: Self = Self {
        outcomes: ReturnOutcomes::NONE,
        entry: None,
        mixed_entries: false,
    };

    fn insert(&mut self, return_: Return) {
        self.outcomes.insert(return_.mode.outcome());
        match self.entry {
            None => self.entry = Some(return_.mode.entry()),
            Some(entry) if entry != return_.mode.entry() => self.mixed_entries = true,
            Some(_) => {}
        }
    }

    fn is(self, outcomes: ReturnOutcomes, entry: ReturnEntry) -> bool {
        self.outcomes == outcomes && self.entry == Some(entry) && !self.mixed_entries
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DepthRoute {
    Caller,
    RoutedMatchOnly(i32),
    RoutedSplit(i32),
}

impl DepthRoute {
    fn expected(self, outcome: plotnik_rt::ReturnOutcome) -> Option<i32> {
        match (self, outcome) {
            (Self::Caller, plotnik_rt::ReturnOutcome::Matched) => Some(0),
            (Self::RoutedMatchOnly(depth), plotnik_rt::ReturnOutcome::Matched)
            | (Self::RoutedSplit(depth), plotnik_rt::ReturnOutcome::Matched) => Some(depth),
            (Self::RoutedSplit(_), plotnik_rt::ReturnOutcome::Empty) => Some(0),
            (Self::Caller | Self::RoutedMatchOnly(_), plotnik_rt::ReturnOutcome::Empty) => None,
        }
    }

    fn matched_depth(self) -> i32 {
        match self {
            Self::Caller => 0,
            Self::RoutedMatchOnly(depth) | Self::RoutedSplit(depth) => depth,
        }
    }

    fn return_entry(self) -> ReturnEntry {
        match self {
            Self::Caller => ReturnEntry::Caller,
            Self::RoutedMatchOnly(_) | Self::RoutedSplit(_) => ReturnEntry::Routed,
        }
    }
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

fn read_operand_u16(storage: &[u8], off: usize) -> Result<u16, ModuleError> {
    storage
        .get(off..off + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or(ModuleError::MalformedInstructionStream)
}

impl Module {
    pub(super) fn load_storage(storage: ByteStorage) -> Result<Self, ModuleError> {
        if storage.len() < HEADER_SIZE {
            return Err(ModuleError::BufferTooSmall(storage.len()));
        }

        let header = Header::from_bytes(&storage[..HEADER_SIZE]);

        if !header.has_valid_magic() {
            return Err(ModuleError::InvalidMagic);
        }
        if !header.is_supported_version() {
            return Err(ModuleError::UnsupportedVersion(header.version));
        }
        if header.total_size as usize != storage.len() {
            return Err(ModuleError::SizeMismatch {
                header: header.total_size,
                actual: storage.len(),
            });
        }

        // Bound every section against the buffer in u64 *before* `compute_offsets`
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
            decoded: DecodedProgram::default(),
            #[cfg(debug_assertions)]
            instr_start_bitmap: Vec::new(),
        };
        // Validation deserializes every regex DFA to prove it well-formed and
        // builds the instruction-start bitmap; it hands the owned automata back so
        // the VM reuses them instead of re-deserializing per evaluation (#426).
        let (regex_dfas, is_start) = module.validate()?;
        module.regex_dfas = regex_dfas;
        // Retain the start bitmap only in debug builds, where it backs the VM's
        // pre-decode IP assertion; release carries no extra per-module memory.
        #[cfg(debug_assertions)]
        {
            module.instr_start_bitmap = is_start;
        }
        #[cfg(not(debug_assertions))]
        let _ = is_start;
        module.decoded = decoded::build(module.instructions_slice());
        Ok(module)
    }

    /// Validate compiler output so later *view* accesses cannot panic and
    /// accidental corruption of the body is detected.
    ///
    /// Section bounds are checked earlier, in [`validate_section_bounds`], before
    /// offsets are computed. The remaining checks here defend a corrupted header
    /// (which the CRC does not cover) whose counts/sizes would otherwise drive
    /// out-of-bounds slicing: the reserved bytes are zero, the CRC32 over the
    /// post-header body matches, the string/regex table sentinels are
    /// well-formed, the documented TypeDef member ranges stay in bounds, and
    /// entry point targets address real instructions.
    ///
    /// The CRC32 detects accidental corruption between emission and
    /// construction. It is not a substitute for structural validation, so a
    /// test buffer with a recomputed checksum must still fail the checks below;
    /// [`Self::validate_instructions`] therefore re-verifies the lazily-decoded
    /// instruction stream structurally, and
    /// [`validate_effect_stack`](super::effect_stack::validate_effect_stack)
    /// proves no path can panic the materializer's builder stack or the VM's
    /// suppression counter — so validated bytecode never panics on view/decode/VM
    /// access even when compiler output is malformed.
    fn validate(&self) -> Result<(RegexDfas, Vec<bool>), ModuleError> {
        // Reserved header bytes are not covered by the CRC; v6 fixes them at zero.
        if self.header._reserved != [0u8; 20] {
            return Err(ModuleError::MalformedHeader);
        }

        let computed = crc32fast::hash(&self.storage[HEADER_SIZE..]);
        if computed != self.header.checksum {
            return Err(ModuleError::ChecksumMismatch {
                expected: self.header.checksum,
                actual: computed,
            });
        }

        self.validate_section_padding()?;
        self.validate_string_table()?;
        self.validate_regex_table()?;
        let regex_dfas = self.load_regex_dfas()?;
        self.validate_type_defs()?;
        self.validate_type_names()?;
        self.validate_spans()?;
        // Bound every embedded `StringId` before any later check constructs a
        // (`NonZero`) `StringId` from one — e.g. `validate_entry_points` builds an
        // `EntryPoint`, which would otherwise panic on a malformed zero name.
        self.validate_string_ids()?;
        self.validate_symbol_ids()?;
        let is_start = self.validate_instructions()?;
        self.validate_entry_points(&is_start)?;
        self.validate_return_routes()?;
        self.validate_depth_neutrality()?;
        // Structural validity (every instruction decodes, every jump lands on a start)
        // is now established, so the effect-stack walk can use the safe typed
        // instruction API. This closes the last malformed-representation panic
        // class: the materializer's builder-stack panics and the VM's
        // suppression underflow.
        super::effect_stack::validate_effect_stack(self)?;
        Ok((regex_dfas, is_start))
    }

    /// Recompute the section layout in `u64` (no overflow) and ensure every
    /// section, up to and including the final one (Spans), fits inside the buffer.
    ///
    /// Runs on the raw header *before* [`Header::compute_offsets`], so a corrupt
    /// header cannot drive that u32 arithmetic to overflow. Sections are laid
    /// out consecutively with alignment padding, so verifying the final
    /// section's end also bounds every earlier section. Passing this check
    /// also proves the `u32` [`SectionOffsets`] will not wrap, so the view
    /// methods can trust them.
    fn validate_section_bounds(h: &Header) -> Result<(), ModuleError> {
        let total = h.total_size;
        let align = SECTION_ALIGN as u64;
        let oob = || ModuleError::SectionOutOfBounds { total };

        let sizes = h.section_data_sizes();
        let (last, rest) = sizes
            .split_last()
            .expect("section layout has at least one section");

        // Every section but the last is alignment-padded; folding them leaves
        // the cursor at the start of the final section, whose unaligned end
        // bounds the buffer.
        let mut cursor = HEADER_SIZE as u64; // sections begin right after the header
        for &size in rest {
            cursor = align_up_u64(cursor + size, align);
        }
        let end = cursor + last;

        if end > total as u64 {
            return Err(oob());
        }
        Ok(())
    }

    /// Every inter-section alignment gap and the final tail up to `total_size`
    /// must be zero. The emitter aligns each section to `SECTION_ALIGN` by
    /// zero-filling the gap before it (`pad_to_section`), so a non-zero byte at a
    /// section boundary is smuggled state riding a gap the CRC alone would carry.
    /// Section bounds are already proven by [`Self::validate_section_bounds`], so
    /// the slicing here stays in range.
    fn validate_section_padding(&self) -> Result<(), ModuleError> {
        let starts = self.offsets.as_starts();
        let sizes = self.header.section_data_sizes();

        // The gap after each section's data, up to the next section's start (or
        // the buffer end for the last section), must be all zero.
        for i in 0..starts.len() {
            let gap_start = (starts[i] + sizes[i] as u32) as usize;
            let gap_end = match starts.get(i + 1) {
                Some(&next) => next as usize,
                None => self.header.total_size as usize,
            };
            if self.storage[gap_start..gap_end].iter().any(|&b| b != 0) {
                return Err(ModuleError::NonZeroSectionPadding);
            }
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
    /// at the blob length, so [`RegexView::at`] never slices OOB.
    fn validate_regex_table(&self) -> Result<(), ModuleError> {
        let table = self.regex_table_slice();
        let blob_len = self.header.regex_blob_size;
        let count = self.header.regex_table_count as usize;

        let mut prev = 0u32;
        for i in 0..=count {
            // Entry layout: string_id (u16) | reserved (u16) | offset (u32).
            // The reserved u16 is pinned to zero (docs/binary-format/03-symbols.md);
            // a non-zero value is smuggled state.
            if read_u16_le(table, i * 8 + 2) != 0 {
                return Err(ModuleError::MalformedRegexTable);
            }
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
    /// [`deserialize_dfa`](crate::bytecode::deserialize_dfa) (`DFA::from_bytes`) validates
    /// the whole serialized automaton — rejecting empty or corrupt bytes — so
    /// this single pass *is* the validation: a deserializable DFA for every real
    /// entry is the invariant the VM's hot predicate path relies on, after which
    /// `RegexDfas::is_match` searches the cached automaton with no further
    /// deserialization. The owned copy detaches the DFA from `self.storage`, so
    /// the cache lives in `Module` without a self-referential borrow. Index 0 is
    /// the reserved sentinel — never evaluated — so it stays `None` and the scan
    /// starts at 1. The offset table is already bounded by
    /// [`Self::validate_regex_table`], so `at` here cannot slice out of
    /// range.
    fn load_regex_dfas(&self) -> Result<RegexDfas, ModuleError> {
        let regexes = self.regexes();
        let count = self.header.regex_table_count as usize;
        let mut dfas = Vec::with_capacity(count);
        dfas.push(None); // index 0: reserved sentinel, never evaluated
        for i in 1..count {
            let bytes = regexes.at(i);
            let dfa = crate::bytecode::deserialize_dfa(bytes)
                .map_err(|_| ModuleError::InvalidRegexDfa(i))?
                .to_owned();
            dfas.push(Some(dfa));
        }
        Ok(RegexDfas::new(dfas))
    }

    /// Validate every TypeDef: a known kind, member runs that stay inside the
    /// TypeMembers section, and every referenced TypeId — a wrapper/alias inner
    /// type or a record/variant member type — addressing a real def, so the
    /// materializer never resolves a type out of range
    /// (`docs/binary-format/04-types.md`).
    fn validate_type_defs(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let members = self.header.type_members_count as u32;
        let type_defs = self.header.type_defs_count;

        for i in 0..types.defs_count() {
            let invalid = || ModuleError::InvalidTypeDef(i);
            let def = types.def(i);
            // Reject an unknown kind here, so the typed reads below cannot panic.
            let Some(data) = def.try_decode() else {
                return Err(invalid());
            };
            // Fields the kind does not name are reserved-zero
            // (docs/binary-format/04-types.md); smuggled state there must not pass validation.
            let (raw_data, raw_count) = def.member_range();
            match data {
                TypeDefKind::Primitive(_) => {
                    if raw_data != 0 || raw_count != 0 {
                        return Err(invalid());
                    }
                }
                TypeDefKind::Wrapper { inner, .. } => {
                    if raw_count != 0 || u16::from(inner) >= type_defs {
                        return Err(invalid());
                    }
                }
                TypeDefKind::Record {
                    member_start,
                    member_count,
                }
                | TypeDefKind::Variant {
                    member_start,
                    member_count,
                } => {
                    // Member-run bounds are identical for record and variant types.
                    if member_start as u32 + member_count as u32 > members {
                        return Err(invalid());
                    }
                    let start = member_start as usize;
                    if (start..start + member_count as usize)
                        .any(|m| u16::from(types.member_type_id(m)) >= type_defs)
                    {
                        return Err(invalid());
                    }
                }
            }
        }
        Ok(())
    }

    /// Every TypeNameEntry must target a real TypeDef; its name `StringId` is checked
    /// separately by [`validate_string_ids`](Self::validate_string_ids).
    fn validate_type_names(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let type_defs = self.header.type_defs_count;
        for i in 0..types.names_count() {
            if u16::from(types.name_type_id(i)) >= type_defs {
                return Err(ModuleError::InvalidTypeName(i));
            }
        }
        Ok(())
    }

    fn validate_spans(&self) -> Result<(), ModuleError> {
        if self.header.spans_count as usize > MAX_SPANS {
            return Err(ModuleError::InvalidSpanEntry(0));
        }

        let bytes = self.spans_slice();
        let type_defs = self.header.type_defs_count;
        let type_members = self.header.type_members_count;
        for i in 0..self.header.spans_count as usize {
            let off = i * SPAN_ENTRY_SIZE;
            let entry = &bytes[off..off + SPAN_ENTRY_SIZE];
            if SpanKind::try_from_u8(entry[2]).is_none() || entry[3] != 0 {
                return Err(ModuleError::InvalidSpanEntry(i));
            }

            let start = u32::from_le_bytes([entry[4], entry[5], entry[6], entry[7]]);
            let end = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]);
            if start > end {
                return Err(ModuleError::InvalidSpanEntry(i));
            }

            let type_id = u16::from_le_bytes([entry[12], entry[13]]);
            let member = u16::from_le_bytes([entry[14], entry[15]]);
            if type_id != SPAN_NO_BINDING && type_id >= type_defs {
                return Err(ModuleError::InvalidSpanEntry(i));
            }
            if member != SPAN_NO_BINDING && member >= type_members {
                return Err(ModuleError::InvalidSpanEntry(i));
            }
            // A member without a type is smuggled state: the emitter never
            // writes it and every consumer keys the binding off `type_id`.
            if type_id == SPAN_NO_BINDING && member != SPAN_NO_BINDING {
                return Err(ModuleError::InvalidSpanEntry(i));
            }
        }
        Ok(())
    }

    /// Entry-point targets must address a real instruction so the VM's first
    /// [`decode_instruction`](Self::decode_instruction) cannot read out of bounds.
    /// `is_start` is the instruction-start bitmap from
    /// [`Self::validate_instructions`]: a `target` that lands inside a multi-word
    /// instruction would make the VM start decoding mid-instruction, so it must
    /// be an instruction start, not merely in range.
    fn validate_entry_points(&self, is_start: &[bool]) -> Result<(), ModuleError> {
        let entry_points = self.entry_points();
        let word_count = self.header.instruction_word_count;
        let type_defs = self.header.type_defs_count;
        let storage: &[u8] = &self.storage;
        let base = self.offsets.entry_points as usize;
        for i in 0..entry_points.len() {
            let invalid = || ModuleError::InvalidEntryPoint(i);
            let ep = entry_points.get(i);
            let target = ep.target();

            if target.get() >= word_count {
                return Err(invalid());
            }

            if !is_start[target.as_usize()] {
                return Err(invalid());
            }

            if u16::from(ep.result_type()) >= type_defs {
                return Err(invalid());
            }

            // Bytes 6-7 are the reserved `_pad`; `EntryPoint::from_bytes` discards
            // them, so a malformed non-zero pad would otherwise pass unnoticed.
            if read_u16_le(storage, base + i * 8 + 6) != 0 {
                return Err(invalid());
            }
        }
        Ok(())
    }

    /// Every entry body returns at the cursor depth promised by its call form.
    ///
    /// The VM treats call bodies as independent cursors: a `Call` applies its own
    /// navigation, then resumes at `next` after a net-neutral callee returns.
    /// A `SplitCall` routes its entry navigation through the callee instead, so
    /// its body and both continuations have that navigation's depth delta.
    fn validate_depth_neutrality(&self) -> Result<(), ModuleError> {
        let mut roots = HashMap::new();

        for entry_point in self.entry_points().iter() {
            let target = entry_point.target();
            let route = DepthRoute::Caller;
            if let Some(expected) = roots.insert(target, route)
                && expected != route
            {
                return Err(ModuleError::MalformedInstructionStream);
            }
        }

        let mut addr = CodeAddr::ZERO;
        while addr.get() < self.header.instruction_word_count {
            let words = match self.decode_instruction(addr) {
                Instruction::Match(m) => m.word_count(),
                Instruction::Call(c) => {
                    let target = CodeAddr::from(u16::from(c.target));
                    let route = DepthRoute::Caller;
                    if let Some(expected) = roots.insert(target, route)
                        && expected != route
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::RoutedCall(c) => {
                    let target = CodeAddr::from(u16::from(c.target));
                    let route = DepthRoute::RoutedMatchOnly(c.entry_nav.depth_delta());
                    if let Some(previous) = roots.insert(target, route)
                        && previous != route
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::SplitCall(c) => {
                    let target = CodeAddr::from(u16::from(c.target));
                    let route = DepthRoute::RoutedSplit(c.entry_nav.depth_delta());
                    if let Some(previous) = roots.insert(target, route)
                        && previous != route
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::Return(_) => 1,
            };
            addr = addr
                .checked_add(words)
                .expect("validated instruction stream fits in u16 address space");
        }

        for (root, route) in roots {
            self.validate_depth_root(root, route)?;
        }
        Ok(())
    }

    /// Calls and callees must agree on the return outcomes carried by their
    /// frames. This keeps malformed bytecode from selecting a continuation an
    /// ordinary frame does not own, or from encoding a split call whose empty
    /// route can never be taken.
    fn validate_return_routes(&self) -> Result<(), ModuleError> {
        let mut cache = HashMap::new();

        for entry_point in self.entry_points().iter() {
            let target = entry_point.target();
            if !self
                .return_contract(target, &mut cache)
                .is(ReturnOutcomes::MATCHED, ReturnEntry::Caller)
            {
                return Err(ModuleError::MalformedInstructionStream);
            }
        }

        let mut addr = CodeAddr::ZERO;
        while addr.get() < self.header.instruction_word_count {
            let words = match self.decode_instruction(addr) {
                Instruction::Match(matched) => matched.word_count(),
                Instruction::Call(call) => {
                    let target = CodeAddr::from(u16::from(call.target));
                    if !self
                        .return_contract(target, &mut cache)
                        .is(ReturnOutcomes::MATCHED, ReturnEntry::Caller)
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::RoutedCall(call) => {
                    let target = CodeAddr::from(u16::from(call.target));
                    if !self
                        .return_contract(target, &mut cache)
                        .is(ReturnOutcomes::MATCHED, ReturnEntry::Routed)
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::SplitCall(call) => {
                    let target = CodeAddr::from(u16::from(call.target));
                    if !self
                        .return_contract(target, &mut cache)
                        .is(ReturnOutcomes::BOTH, ReturnEntry::Routed)
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    1
                }
                Instruction::Return(_) => 1,
            };
            addr = addr
                .checked_add(words)
                .expect("validated instruction stream fits in u16 address space");
        }
        Ok(())
    }

    fn return_contract(
        &self,
        entry: CodeAddr,
        cache: &mut HashMap<CodeAddr, ReturnContract>,
    ) -> ReturnContract {
        if let Some(&contract) = cache.get(&entry) {
            return contract;
        }

        let mut contract = ReturnContract::NONE;
        let mut seen = HashSet::new();
        let mut work = vec![entry];
        while let Some(addr) = work.pop() {
            if !seen.insert(addr) {
                continue;
            }
            match self.decode_instruction(addr) {
                Instruction::Match(matched) => {
                    work.extend(
                        matched
                            .successors()
                            .map(|next| CodeAddr::from(u16::from(next))),
                    );
                }
                Instruction::Call(call) => work.push(CodeAddr::from(u16::from(call.next))),
                Instruction::RoutedCall(call) => {
                    work.push(CodeAddr::from(u16::from(call.next)));
                }
                Instruction::SplitCall(call) => {
                    work.push(CodeAddr::from(u16::from(call.returns.empty)));
                    work.push(CodeAddr::from(u16::from(call.returns.matched)));
                }
                Instruction::Return(return_) => contract.insert(return_),
            }
        }
        cache.insert(entry, contract);
        contract
    }

    fn validate_depth_root(&self, entry: CodeAddr, route: DepthRoute) -> Result<(), ModuleError> {
        let mut memo: HashMap<CodeAddr, i32> = HashMap::new();
        let mut work = vec![(entry, 0i32)];

        while let Some((addr, net)) = work.pop() {
            if let Some(&seen) = memo.get(&addr) {
                if seen == net {
                    continue;
                }
                return Err(ModuleError::DepthImbalance(addr));
            }
            memo.insert(addr, net);

            match self.decode_instruction(addr) {
                Instruction::Return(return_) => {
                    if return_.mode.entry() != route.return_entry() {
                        return Err(ModuleError::DepthImbalance(addr));
                    }
                    let Some(expected_exit) = route.expected(return_.mode.outcome()) else {
                        return Err(ModuleError::DepthImbalance(addr));
                    };
                    if net != expected_exit {
                        return Err(ModuleError::DepthImbalance(addr));
                    }
                }
                Instruction::Match(m) => {
                    let next_net = net + m.nav.depth_delta();
                    if m.succ_count() == 0 {
                        let expected_exit = route.matched_depth();
                        if next_net != expected_exit {
                            return Err(ModuleError::DepthImbalance(addr));
                        }
                    } else {
                        for succ in m.successors() {
                            work.push((CodeAddr::from(u16::from(succ)), next_net));
                        }
                    }
                }
                Instruction::Call(c) => {
                    work.push((CodeAddr::from(u16::from(c.next)), net + c.nav.depth_delta()));
                }
                Instruction::RoutedCall(c) => {
                    work.push((
                        CodeAddr::from(u16::from(c.next)),
                        net + c.entry_nav.depth_delta(),
                    ));
                }
                Instruction::SplitCall(c) => {
                    work.push((
                        CodeAddr::from(u16::from(c.returns.matched)),
                        net + c.entry_nav.depth_delta(),
                    ));
                    work.push((CodeAddr::from(u16::from(c.returns.empty)), net));
                }
            }
        }

        Ok(())
    }

    /// Every *required* `StringId` held in a section — entry point names,
    /// node/field symbol names, type names, type member names, and regex pattern
    /// names — must address a real string-table entry, so the view accessors that
    /// resolve them (and `find_by_name`, the materializer's record-field keys,
    /// etc.) never slice out of bounds. The table holds `str_table_count + 1`
    /// offsets, so the valid id range is `0..str_table_count`. This upholds the
    /// representation's guarantee that validated bytecode never panics on view access
    /// (`docs/binary-format/01-overview.md`).
    fn validate_string_ids(&self) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let n = self.header.str_table_count;

        // Read the raw `u16` rather than the typed accessor: a required `StringId`
        // is a `NonZeroU16`, so building one from a malformed zero would panic
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

        // Entry-point name: u16 at entry+0
        check(
            self.offsets.entry_points,
            EntryPoint::SIZE,
            0,
            0,
            self.header.entry_points_count as usize,
        )?;
        // node/field symbol name: u16 at entry+2
        check(
            self.offsets.node_kinds,
            SymbolNameEntry::SIZE,
            2,
            0,
            self.header.node_kinds_count as usize,
        )?;
        check(
            self.offsets.node_fields,
            SymbolNameEntry::SIZE,
            2,
            0,
            self.header.node_fields_count as usize,
        )?;
        // type name / member name: u16 at entry+0
        check(
            self.offsets.type_names,
            TypeNameEntry::SIZE,
            0,
            0,
            self.header.type_names_count as usize,
        )?;
        check(
            self.offsets.type_members,
            TypeMember::SIZE,
            0,
            0,
            self.header.type_members_count as usize,
        )?;
        // regex pattern name: u16 at entry+0. Index 0 is the reserved sentinel —
        // never resolved — so start at 1; `dump`/`trace` resolve `string_id` for
        // every real entry through the panicking `RegexView::pattern_string_id` (and
        // then index the string blob).
        check(
            self.offsets.regex_table,
            REGEX_TABLE_ENTRY_SIZE,
            0,
            1,
            self.header.regex_table_count as usize,
        )?;
        Ok(())
    }

    /// The `symbol` half of each node-kind/node-field entry must be non-zero:
    /// renderers rebuild `NodeKindId`/`NodeFieldId` (`NonZeroU16`) from it via
    /// `try_from(..).expect(..)` (`render.rs`), so a malformed zero would panic
    /// `dump`/`trace` instead of failing validation.
    fn validate_symbol_ids(&self) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let check = |base: u32, count: usize| {
            let base = base as usize;
            for i in 0..count {
                if read_u16_le(storage, base + i * SymbolNameEntry::SIZE) == 0 {
                    return Err(ModuleError::InvalidNodeSymbol(i));
                }
            }
            Ok(())
        };
        check(
            self.offsets.node_kinds,
            self.header.node_kinds_count as usize,
        )?;
        check(
            self.offsets.node_fields,
            self.header.node_fields_count as usize,
        )?;
        Ok(())
    }

    /// Structurally re-verify the whole instruction stream so the documented
    /// guarantee — validated bytecode never panics on view/decode access — holds
    /// for every compiler buffer whose header and CRC check out, including the
    /// deliberately mutated buffers used by validation tests.
    ///
    /// A module is decoded lazily: [`decode_instruction`](Self::decode_instruction) and the
    /// per-opcode decoders, the effect/predicate iterators, and the materializer
    /// all build `NonZero`/enum values and index tables straight from
    /// instruction bytes. Each is a panic site on malformed compiler output — `Opcode`,
    /// `Nav`, `NodeKindConstraint`, `EffectKind`, and `SuccessorAddr` decoding, plus
    /// `get_member` / `at` table lookups. This walk rejects every such
    /// input up front, reading only through checked slicing so it never panics
    /// itself.
    ///
    /// Two passes over the stream:
    /// 1. Decode each instruction's fixed-size slot (the slot size is fixed by
    ///    the opcode, so the walk is unambiguous), validating opcode, segment,
    ///    nav, node kind, effect opcodes, `RecordSet`/`Variant` member operands, and
    ///    predicate operands, and rejecting any zero successor address. Record
    ///    each instruction start and collect every jump target.
    /// 2. Every collected jump target — successor or call next/target — must land
    ///    on a recorded instruction start.
    ///
    /// Returns the instruction-start bitmap so [`Self::validate_entry_points`] can
    /// hold entry point targets to the same rule: an entry point pointing into the
    /// interior of a multi-word instruction would otherwise begin decoding
    /// mid-instruction.
    ///
    /// Out of scope (not a decode/view panic): node-kind/field ids, which are
    /// resolved against the tree-sitter grammar at match time, and member
    /// `type_id`s, which the materializer reads through the checked `Types::get`
    /// that returns `Option`.
    fn validate_instructions(&self) -> Result<Vec<bool>, ModuleError> {
        let storage: &[u8] = &self.storage;
        let base = self.offsets.instructions as usize;
        let word_count = self.header.instruction_word_count;

        let read_u8 = |off: usize| {
            storage
                .get(off)
                .copied()
                .ok_or(ModuleError::MalformedInstructionStream)
        };
        // A reserved padding run inside an instruction slot must be all zero.
        let check_zero = |off: usize, len: usize| match storage.get(off..off + len) {
            Some(run) if run.iter().all(|&b| b == 0) => Ok(()),
            _ => Err(ModuleError::MalformedInstructionStream),
        };

        let mut is_start = vec![false; word_count as usize];
        let mut targets: Vec<CodeAddr> = Vec::new();

        let mut addr = CodeAddr::ZERO;
        while addr.get() < word_count {
            is_start[addr.as_usize()] = true;
            let instr_off = base + addr.as_usize() * BYTECODE_WORD_SIZE;
            let header = read_u8(instr_off)?;

            let nibble = header_byte::opcode_nibble(header);
            let Some(opcode) = Opcode::from_u8(nibble) else {
                return Err(ModuleError::InvalidOpcode {
                    addr,
                    opcode: nibble,
                });
            };
            // Every opcode reserves the segment bits; the call/return
            // decoders `assert!` segment == 0, and a non-zero segment is unused.
            if header_byte::segment(header) != 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
            // node_class_bits (header bits 4-5) is meaningful only for Match
            // variants; Call/Return ignore it, so the format pins those bits to
            // zero — a malformed non-zero node_class_bits there is smuggled state.
            if matches!(
                opcode,
                Opcode::Call | Opcode::RoutedCall | Opcode::Return | Opcode::SplitCall
            ) && header_byte::node_class_bits(header) != 0
            {
                return Err(ModuleError::MalformedInstructionStream);
            }

            match opcode {
                Opcode::Return => {
                    if plotnik_rt::ReturnOutcome::from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    if ReturnEntry::from_byte(read_u8(instr_off + 2)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    check_zero(instr_off + 3, 5)?;
                }
                Opcode::Call => {
                    // `Call::from_bytes` decodes a nav and two non-zero successor addresses.
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let next = read_operand_u16(storage, instr_off + 4)?;
                    let target = read_operand_u16(storage, instr_off + 6)?;
                    if next == 0 || target == 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    targets.push(CodeAddr::from(next));
                    targets.push(CodeAddr::from(target));
                }
                Opcode::RoutedCall => {
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    check_zero(instr_off + 2, 2)?;
                    let next = read_operand_u16(storage, instr_off + 4)?;
                    let target = read_operand_u16(storage, instr_off + 6)?;
                    if next == 0 || target == 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    targets.push(CodeAddr::from(next));
                    targets.push(CodeAddr::from(target));
                }
                Opcode::SplitCall => {
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let matched = read_operand_u16(storage, instr_off + 2)?;
                    let empty = read_operand_u16(storage, instr_off + 4)?;
                    let target = read_operand_u16(storage, instr_off + 6)?;
                    if matched == 0 || empty == 0 || target == 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    targets.extend([
                        CodeAddr::from(matched),
                        CodeAddr::from(empty),
                        CodeAddr::from(target),
                    ]);
                }
                opcode if opcode.is_match() => {
                    // A Match variant (`Match8` or extended).
                    let node_kind = header_byte::node_class_bits(header);
                    if NodeKindConstraint::try_from_bytes(
                        node_kind,
                        read_operand_u16(storage, instr_off + 2)?,
                    )
                    .is_none()
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    if Nav::try_from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }

                    if opcode == Opcode::Match8 {
                        // bytes 6-7 hold the single successor; `0` means terminal.
                        let next = read_operand_u16(storage, instr_off + 6)?;
                        if next != 0 {
                            targets.push(CodeAddr::from(next));
                        }
                    } else {
                        self.validate_extended_match(opcode, instr_off, addr, &mut targets)?;
                    }
                }
                _ => unreachable!("all non-match opcodes handled above"),
            }

            addr = addr
                .checked_add(opcode.word_count())
                .ok_or(ModuleError::MalformedInstructionStream)?;
        }

        // A well-formed stream tiles the section in whole instructions. An
        // overrun means a trailing instruction's slot crosses the section end,
        // so a successor pointing into it could later decode past the buffer.
        if addr.get() != word_count {
            return Err(ModuleError::MalformedInstructionStream);
        }

        for target in targets {
            if target.get() >= word_count || !is_start[target.as_usize()] {
                return Err(ModuleError::MalformedInstructionStream);
            }
        }
        Ok(is_start)
    }

    /// Validate the payload of one extended `Match` (`Match16`..`Match64`):
    /// effects, predicate, and successors. Appends each successor to `targets`
    /// for the pass-2 jump-target check in [`Self::validate_instructions`].
    fn validate_extended_match(
        &self,
        opcode: Opcode,
        instr_off: usize,
        addr: CodeAddr,
        targets: &mut Vec<CodeAddr>,
    ) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;

        let counts = read_operand_u16(storage, instr_off + 6)?;
        // Bits 1-0 of the counts word are reserved (bit 2 is the `missing` flag,
        // which the decoder does read); the decoder never reads the reserved bits,
        // so a malformed set bit would pass validation unnoticed.
        if MatchCounts::reserved_bits_set(counts) {
            return Err(ModuleError::MalformedInstructionStream);
        }
        let c = MatchCounts::unpack(counts);
        let effects = c.effects as usize;
        let neg = c.neg as usize;
        let succ = c.succ as usize;
        let has_predicate = c.has_predicate;

        // Every payload slot the decoders read — effects, predicate, successors —
        // must lie within this instruction's fixed-size slot, or the iterators
        // read into the next instruction (or past the buffer at the stream end).
        let used = effects + neg + if has_predicate { PREDICATE_SLOTS } else { 0 } + succ;
        if used > opcode.payload_slots() {
            return Err(ModuleError::MalformedInstructionStream);
        }

        // Every decoded effect validates its payload through `EffectKind`'s
        // metadata contract before any trusting consumer sees it.
        let members = usize::from(self.header.type_members_count);
        let spans = usize::from(self.header.spans_count);
        let check_effect = |slot: usize| -> Result<(), ModuleError> {
            let off = instr_off + MATCH_PAYLOAD_START + slot * PAYLOAD_SLOT_SIZE;
            let b = storage
                .get(off..off + PAYLOAD_SLOT_SIZE)
                .ok_or(ModuleError::MalformedInstructionStream)?;
            let op = Effect::try_from_bytes([b[0], b[1]])
                .ok_or(ModuleError::MalformedInstructionStream)?;
            if op.kind.accepts_payload(op.payload, members, spans) {
                return Ok(());
            }
            if matches!(
                op.kind,
                EffectKind::SpanStartAt | EffectKind::SpanStart | EffectKind::SpanEnd
            ) {
                return Err(ModuleError::InvalidSpanPayload(addr));
            }
            Err(ModuleError::MalformedInstructionStream)
        };
        for i in 0..effects {
            check_effect(i)?;
        }

        // Neg-field slots hold raw `NodeFieldId`s (`NonZeroU16`); the decoder's
        // `neg_fields()` rebuilds them via `try_from(..).expect(..)`
        // (`instructions.rs`), so a malformed zero must not pass validation.
        for i in 0..neg {
            let off = instr_off + MATCH_PAYLOAD_START + (effects + i) * PAYLOAD_SLOT_SIZE;
            let b = storage
                .get(off..off + PAYLOAD_SLOT_SIZE)
                .ok_or(ModuleError::MalformedInstructionStream)?;
            if u16::from_le_bytes([b[0], b[1]]) == 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
        }

        if has_predicate {
            let pred_off = instr_off + MATCH_PAYLOAD_START + (effects + neg) * PAYLOAD_SLOT_SIZE;
            let b = storage
                .get(pred_off..pred_off + PREDICATE_SIZE)
                .ok_or(ModuleError::MalformedInstructionStream)?;
            let op_and_flags = u16::from_le_bytes([b[0], b[1]]);
            let (op, is_regex) = MatchPredicate::unpack_op_flags(op_and_flags);
            let value_ref = u16::from_le_bytes([b[2], b[3]]);

            // Bits above the operator and regex flag are reserved-zero
            // (docs/binary-format/06-instructions.md), so a malformed set bit must
            // not pass validation.
            if MatchPredicate::reserved_bits_set(op_and_flags) {
                return Err(ModuleError::InvalidPredicateOperand(addr));
            }

            // The operator must be a known predicate op, the regex flag must agree
            // with the operator's class, and the operand must index its table —
            // otherwise `PredicateOp::from_byte`, `at`, or the VM's
            // op/flag `unreachable!` would panic when this predicate is evaluated
            // or dumped. The regex operand must be a *real* entry (`1..count`):
            // index 0 is the reserved sentinel that `load_regex_dfas` leaves empty,
            // so its DFA slot is `None`, and the VM `.expect()`s a populated slot.
            // A string operand of 0 is benign — the validated easter-egg entry,
            // never asserted non-empty.
            let Some(pred_op) = PredicateOp::try_from_byte(op) else {
                return Err(ModuleError::InvalidPredicateOperand(addr));
            };
            let operand_ok = if is_regex {
                (1..self.header.regex_table_count).contains(&value_ref)
            } else {
                value_ref < self.header.str_table_count
            };
            if pred_op.is_regex_op() != is_regex || !operand_ok {
                return Err(ModuleError::InvalidPredicateOperand(addr));
            }
        }

        let succ_off = instr_off
            + MATCH_PAYLOAD_START
            + (effects + neg) * PAYLOAD_SLOT_SIZE
            + if has_predicate { PREDICATE_SIZE } else { 0 };
        for i in 0..succ {
            let next = read_operand_u16(storage, succ_off + i * PAYLOAD_SLOT_SIZE)?;
            // An extended successor decodes through `SuccessorAddr`, which panics
            // on zero; `0` is the terminal marker only for the `Match8` slot.
            if next == 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
            targets.push(CodeAddr::from(next));
        }
        Ok(())
    }
}
