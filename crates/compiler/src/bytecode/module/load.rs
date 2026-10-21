use super::super::effects::{Effect, EffectKind};
use super::super::instructions::{
    MATCH_PAYLOAD_START, MatchCounts, PAYLOAD_SLOT_SIZE, PREDICATE_SIZE, PREDICATE_SLOTS,
    header_byte,
};
use super::super::node_kind_constraint::NodeKindConstraint;
use super::super::sections::SYMBOL_NAME_ENTRY_SIZE;
use super::super::type_meta::{TypeDefKind, TypeMember, TypeNameEntry};
use super::super::{
    HEADER_SIZE, MAX_SPANS, SECTION_ALIGN, SPAN_ENTRY_SIZE, SPAN_NO_BINDING, SpanKind, VERSION,
};
use super::*;
use plotnik_runtime::{Nav, NodeFieldId, NodeKindId, PortId};

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
    #[error("malformed switch table")]
    MalformedSwitchTable,
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
    #[error("invalid entrypoint at index {0}")]
    InvalidEntrypoint(usize),
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
    #[error("cursor read on an empty path at instruction address {0}")]
    EmptyPathCursorRead(CodeAddr),
    #[error("invalid span entry at index {0}")]
    InvalidSpanEntry(usize),
    #[error("span effect payload out of range at instruction address {0}")]
    InvalidSpanPayload(CodeAddr),
    #[error("span bracket imbalance at instruction address {0}")]
    SpanImbalance(CodeAddr),
}

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

fn is_call_nav(nav: Nav) -> bool {
    matches!(
        nav,
        Nav::Stay
            | Nav::StayExact
            | Nav::Next
            | Nav::NextSkip
            | Nav::NextSkipExtras
            | Nav::NextExact
            | Nav::Down
            | Nav::DownSkip
            | Nav::DownSkipExtras
            | Nav::DownExact
    )
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

        Self::validate_section_bounds(&header)?;

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
        let (regex_dfas, is_start) = module.validate()?;
        module.regex_dfas = regex_dfas;
        #[cfg(debug_assertions)]
        {
            module.instr_start_bitmap = is_start;
        }
        #[cfg(not(debug_assertions))]
        let _ = is_start;
        module.decoded = decoded::build(&module)?;
        Ok(module)
    }

    fn validate(&self) -> Result<(RegexDfas, Vec<bool>), ModuleError> {
        if self.header._reserved != [0u8; 16] {
            return Err(ModuleError::MalformedHeader);
        }

        if !self
            .header
            .switch_tables_size
            .is_multiple_of(SwitchEntry::SIZE as u32)
        {
            return Err(ModuleError::MalformedSwitchTable);
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
        self.validate_string_ids()?;
        self.validate_symbol_ids()?;
        let is_start = self.validate_instructions()?;
        self.validate_entrypoints(&is_start)?;
        super::verify::validate(self)?;
        Ok((regex_dfas, is_start))
    }

    fn validate_section_bounds(h: &Header) -> Result<(), ModuleError> {
        let total = h.total_size;
        let align = SECTION_ALIGN as u64;
        let oob = || ModuleError::SectionOutOfBounds { total };

        let sizes = h.section_data_sizes();
        let (last, rest) = sizes
            .split_last()
            .expect("section layout has at least one section");

        let mut cursor = HEADER_SIZE as u64;
        for &size in rest {
            cursor = align_up_u64(cursor + size, align);
        }
        let end = cursor + last;

        if end > total as u64 {
            return Err(oob());
        }
        Ok(())
    }

    fn validate_section_padding(&self) -> Result<(), ModuleError> {
        let starts = self.offsets.as_starts();
        let sizes = self.header.section_data_sizes();

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

    fn validate_regex_table(&self) -> Result<(), ModuleError> {
        let table = self.regex_table_slice();
        let blob_len = self.header.regex_blob_size;
        let count = self.header.regex_table_count as usize;

        let mut prev = 0u32;
        for i in 0..=count {
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

    fn load_regex_dfas(&self) -> Result<RegexDfas, ModuleError> {
        let regexes = self.regexes();
        let count = self.header.regex_table_count as usize;
        let mut dfas = Vec::with_capacity(count);
        dfas.push(None);
        for i in 1..count {
            let bytes = regexes.at(i);
            let dfa = crate::bytecode::deserialize_dfa(bytes)
                .map_err(|_| ModuleError::InvalidRegexDfa(i))?
                .to_owned();
            dfas.push(Some(dfa));
        }
        Ok(RegexDfas::new(dfas))
    }

    fn validate_type_defs(&self) -> Result<(), ModuleError> {
        let types = self.types();
        let members = self.header.type_members_count as u32;
        let type_defs = self.header.type_defs_count;

        for i in 0..types.defs_count() {
            let invalid = || ModuleError::InvalidTypeDef(i);
            let def = types.def(i);
            let Some(data) = def.try_decode() else {
                return Err(invalid());
            };
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
            if type_id == SPAN_NO_BINDING && member != SPAN_NO_BINDING {
                return Err(ModuleError::InvalidSpanEntry(i));
            }
        }
        Ok(())
    }

    fn validate_entrypoints(&self, is_start: &[bool]) -> Result<(), ModuleError> {
        let entrypoints = self.entrypoints();
        let word_count = self.header.instruction_word_count;
        let type_defs = self.header.type_defs_count;
        let mut has_expression = false;
        for (i, ep) in entrypoints.iter().enumerate() {
            let invalid = || ModuleError::InvalidEntrypoint(i);
            if ep.identity() == EntrypointIdentity::Expression {
                if has_expression {
                    return Err(invalid());
                }
                has_expression = true;
            }

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

            if ep.try_boundary().is_none() {
                return Err(invalid());
            }
        }
        Ok(())
    }

    fn validate_string_ids(&self) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let n = self.header.str_table_count;

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

        for i in 0..self.header.entrypoints_count as usize {
            let identity = read_u16_le(
                storage,
                self.offsets.entrypoints as usize + i * Entrypoint::SIZE,
            );
            if identity != 0 && identity >= n {
                return Err(ModuleError::InvalidStringId(i));
            }
        }

        check(
            self.offsets.node_kinds,
            SYMBOL_NAME_ENTRY_SIZE,
            2,
            0,
            self.header.node_kinds_count as usize,
        )?;
        check(
            self.offsets.node_fields,
            SYMBOL_NAME_ENTRY_SIZE,
            2,
            0,
            self.header.node_fields_count as usize,
        )?;
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
        check(
            self.offsets.regex_table,
            REGEX_TABLE_ENTRY_SIZE,
            0,
            1,
            self.header.regex_table_count as usize,
        )?;
        Ok(())
    }

    fn validate_symbol_ids(&self) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;
        let check = |base: u32, count: usize, node_kinds: bool| {
            let base = base as usize;
            for i in 0..count {
                let symbol = read_u16_le(storage, base + i * SYMBOL_NAME_ENTRY_SIZE);
                let valid = if node_kinds {
                    NodeKindId::try_from(symbol).is_ok_and(NodeKindId::is_regular)
                } else {
                    NodeFieldId::try_from(symbol).is_ok()
                };
                if !valid {
                    return Err(ModuleError::InvalidNodeSymbol(i));
                }
            }
            Ok(())
        };
        check(
            self.offsets.node_kinds,
            self.header.node_kinds_count as usize,
            true,
        )?;
        check(
            self.offsets.node_fields,
            self.header.node_fields_count as usize,
            false,
        )?;
        Ok(())
    }

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
        let check_zero = |off: usize, len: usize| match storage.get(off..off + len) {
            Some(run) if run.iter().all(|&b| b == 0) => Ok(()),
            _ => Err(ModuleError::MalformedInstructionStream),
        };

        let mut is_start = vec![false; word_count as usize];
        let mut targets: Vec<CodeAddr> = Vec::new();

        let switch_tables = self.switch_tables();
        for &bytes in switch_tables.raw {
            let entry =
                SwitchEntry::try_from_bytes(bytes).ok_or(ModuleError::MalformedSwitchTable)?;
            targets.push(CodeAddr::from(u16::from(entry.target)));
        }

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
            if header_byte::segment(header) != 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
            match opcode {
                Opcode::Switch => {
                    if header & 0xf0 != 0 || read_u8(instr_off + 1)? != 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }

                    let switch = Switch::from_bytes(&storage[instr_off..instr_off + 8]);
                    let table = switch_tables
                        .get_run(&switch)
                        .ok_or(ModuleError::MalformedInstructionStream)?;
                    let mut previous = None;
                    for entry in table.iter() {
                        if previous.is_some_and(|prev| prev >= entry.kind) {
                            return Err(ModuleError::MalformedSwitchTable);
                        }

                        previous = Some(entry.kind);
                    }
                }
                Opcode::Return => {
                    if PortId::from_byte(read_u8(instr_off + 1)?).is_none() {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let flags = header_byte::node_class_bits(header);
                    if flags & !1 != 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let Some(nav) = Nav::try_from_byte(read_u8(instr_off + 2)?) else {
                        return Err(ModuleError::MalformedInstructionStream);
                    };
                    if !is_call_nav(nav) {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let node_field = read_operand_u16(storage, instr_off + 4)?;
                    if flags & 1 == 0 && (nav != Nav::Stay || node_field != 0) {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    check_zero(instr_off + 3, 1)?;
                    check_zero(instr_off + 6, 2)?;
                }
                Opcode::Call1 => {
                    let Some(nav) = Nav::try_from_byte(read_u8(instr_off + 1)?) else {
                        return Err(ModuleError::MalformedInstructionStream);
                    };
                    if !is_call_nav(nav) {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let flags = header_byte::node_class_bits(header);
                    let caller_owned = flags & 1 == 0;
                    let consumed = flags & 2 != 0;
                    if caller_owned && !consumed {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let next = read_operand_u16(storage, instr_off + 4)?;
                    let target = read_operand_u16(storage, instr_off + 6)?;
                    if next == 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    targets.push(CodeAddr::from(next));
                    targets.push(CodeAddr::from(target));
                }
                Opcode::CallN => {
                    let Some(nav) = Nav::try_from_byte(read_u8(instr_off + 1)?) else {
                        return Err(ModuleError::MalformedInstructionStream);
                    };
                    if !is_call_nav(nav) {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let flags = header_byte::node_class_bits(header);
                    if flags & !1 != 0 {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let target = read_operand_u16(storage, instr_off + 4)?;
                    let arity = usize::from(read_u8(instr_off + 6)?);
                    if !(2..=usize::from(PortId::COUNT)).contains(&arity) {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    let consumed_mask = read_u8(instr_off + 7)?;
                    let valid_mask = PortId::dense_mask(arity);
                    if consumed_mask & !valid_mask != 0
                        || (flags & 1 == 0 && consumed_mask != valid_mask)
                    {
                        return Err(ModuleError::MalformedInstructionStream);
                    }
                    targets.push(CodeAddr::from(target));
                    for index in 0..usize::from(PortId::COUNT) {
                        let return_addr =
                            read_operand_u16(storage, instr_off + BYTECODE_WORD_SIZE + index * 2)?;
                        if index < arity {
                            if return_addr == 0 {
                                return Err(ModuleError::MalformedInstructionStream);
                            }
                            targets.push(CodeAddr::from(return_addr));
                        } else if return_addr != 0 {
                            return Err(ModuleError::MalformedInstructionStream);
                        }
                    }
                }
                opcode if opcode.is_match() => {
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

    fn validate_extended_match(
        &self,
        opcode: Opcode,
        instr_off: usize,
        addr: CodeAddr,
        targets: &mut Vec<CodeAddr>,
    ) -> Result<(), ModuleError> {
        let storage: &[u8] = &self.storage;

        let counts = read_operand_u16(storage, instr_off + 6)?;
        let c = MatchCounts::unpack(counts);
        let effects = c.effects as usize;
        let neg = c.neg as usize;
        let succ = c.succ as usize;
        let has_predicate = c.predicate.is_some();

        let used = effects + neg + if has_predicate { PREDICATE_SLOTS } else { 0 } + succ;
        if used > opcode.payload_slots() {
            return Err(ModuleError::MalformedInstructionStream);
        }

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

        for i in 0..neg {
            let off = instr_off + MATCH_PAYLOAD_START + (effects + i) * PAYLOAD_SLOT_SIZE;
            let b = storage
                .get(off..off + PAYLOAD_SLOT_SIZE)
                .ok_or(ModuleError::MalformedInstructionStream)?;
            if u16::from_le_bytes([b[0], b[1]]) == 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
        }

        if let Some(op) = c.predicate {
            let pred_off = instr_off + MATCH_PAYLOAD_START + (effects + neg) * PAYLOAD_SLOT_SIZE;
            let value_ref = read_operand_u16(storage, pred_off)?;
            let operand_ok = if op.is_regex_op() {
                (1..self.header.regex_table_count).contains(&value_ref)
            } else {
                value_ref < self.header.str_table_count
            };
            if !operand_ok {
                return Err(ModuleError::InvalidPredicateOperand(addr));
            }
        }

        let succ_off = instr_off
            + MATCH_PAYLOAD_START
            + (effects + neg) * PAYLOAD_SLOT_SIZE
            + if has_predicate { PREDICATE_SIZE } else { 0 };
        for i in 0..succ {
            let next = read_operand_u16(storage, succ_off + i * PAYLOAD_SLOT_SIZE)?;
            if next == 0 {
                return Err(ModuleError::MalformedInstructionStream);
            }
            targets.push(CodeAddr::from(next));
        }
        Ok(())
    }
}
