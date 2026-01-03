//! Bytecode instruction definitions.
//!
//! Instructions are runtime-friendly structs with `from_bytes`/`to_bytes`
//! methods for bytecode serialization.

use std::num::NonZeroU16;

use super::constants::{SECTION_ALIGN, STEP_SIZE};
use super::effects::EffectOp;
use super::ids::StepId;
use super::nav::Nav;

/// Read `count` little-endian u16 values from bytes starting at `offset`.
/// Advances `offset` by `count * 2`.
#[inline]
fn read_u16_vec(bytes: &[u8], offset: &mut usize, count: usize) -> Vec<u16> {
    (0..count)
        .map(|_| {
            let v = u16::from_le_bytes([bytes[*offset], bytes[*offset + 1]]);
            *offset += 2;
            v
        })
        .collect()
}

/// Instruction opcodes (4-bit).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Opcode {
    Match8 = 0x0,
    Match16 = 0x1,
    Match24 = 0x2,
    Match32 = 0x3,
    Match48 = 0x4,
    Match64 = 0x5,
    Call = 0x6,
    Return = 0x7,
}

impl Opcode {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x0 => Self::Match8,
            0x1 => Self::Match16,
            0x2 => Self::Match24,
            0x3 => Self::Match32,
            0x4 => Self::Match48,
            0x5 => Self::Match64,
            0x6 => Self::Call,
            0x7 => Self::Return,
            _ => panic!("invalid opcode: {v}"),
        }
    }

    /// Instruction size in bytes.
    pub fn size(self) -> usize {
        match self {
            Self::Match8 => 8,
            Self::Match16 => 16,
            Self::Match24 => 24,
            Self::Match32 => 32,
            Self::Match48 => 48,
            Self::Match64 => 64,
            Self::Call => 8,
            Self::Return => 8,
        }
    }

    /// Number of steps this instruction occupies.
    pub fn step_count(self) -> u16 {
        (self.size() / STEP_SIZE) as u16
    }

    /// Whether this is a Match variant.
    pub fn is_match(self) -> bool {
        matches!(
            self,
            Self::Match8
                | Self::Match16
                | Self::Match24
                | Self::Match32
                | Self::Match48
                | Self::Match64
        )
    }

    /// Whether this is an extended Match (Match16-64).
    pub fn is_extended_match(self) -> bool {
        matches!(
            self,
            Self::Match16 | Self::Match24 | Self::Match32 | Self::Match48 | Self::Match64
        )
    }

    /// Payload capacity in u16 slots for extended Match variants.
    pub fn payload_slots(self) -> usize {
        match self {
            Self::Match16 => 4,
            Self::Match24 => 8,
            Self::Match32 => 12,
            Self::Match48 => 20,
            Self::Match64 => 28,
            _ => 0,
        }
    }
}

/// Match instruction for pattern matching in the VM.
///
/// Unifies Match8 (fast-path) and Match16-64 (extended) wire formats into
/// a single runtime-friendly struct.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Match {
    /// Segment index (0-15, currently only 0 is used).
    pub segment: u8,
    /// Navigation command.
    pub nav: Nav,
    /// Node type constraint (None = wildcard).
    pub node_type: Option<NonZeroU16>,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NonZeroU16>,
    /// Effects to execute before match attempt.
    pub pre_effects: Vec<EffectOp>,
    /// Fields that must NOT be present on the node.
    pub neg_fields: Vec<u16>,
    /// Effects to execute after successful match.
    pub post_effects: Vec<EffectOp>,
    /// Successor step IDs (empty = accept, 1 = linear, 2+ = branch).
    pub successors: Vec<StepId>,
}

impl Match {
    /// Check if this is a terminal (accept) state.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.successors.is_empty()
    }

    /// Check if this is an epsilon transition (no node interaction).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Stay && self.node_type.is_none() && self.node_field.is_none()
    }

    /// Decode from bytecode bytes.
    ///
    /// The slice must start at the instruction and contain at least
    /// the full instruction size (determined by opcode).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        assert!(bytes.len() >= 8, "Match instruction too short");

        let type_id_byte = bytes[0];
        let segment = type_id_byte >> 4;
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        let opcode = Opcode::from_u8(type_id_byte & 0xF);

        assert!(opcode.is_match(), "expected Match opcode, got {opcode:?}");
        assert!(
            bytes.len() >= opcode.size(),
            "Match instruction truncated: expected {} bytes, got {}",
            opcode.size(),
            bytes.len()
        );

        let nav = Nav::from_byte(bytes[1]);
        let node_type = NonZeroU16::new(u16::from_le_bytes([bytes[2], bytes[3]]));
        let node_field = NonZeroU16::new(u16::from_le_bytes([bytes[4], bytes[5]]));

        if opcode == Opcode::Match8 {
            // Match8: single successor in bytes 6-7 (0 = terminal)
            let next_raw = u16::from_le_bytes([bytes[6], bytes[7]]);
            let successors = NonZeroU16::new(next_raw)
                .map(|n| vec![StepId(n)])
                .unwrap_or_default();

            Self {
                segment,
                nav,
                node_type,
                node_field,
                pre_effects: vec![],
                neg_fields: vec![],
                post_effects: vec![],
                successors,
            }
        } else {
            // Extended match: parse counts and payload
            let counts = u16::from_le_bytes([bytes[6], bytes[7]]);
            let pre_count = ((counts >> 13) & 0x7) as usize;
            let neg_count = ((counts >> 10) & 0x7) as usize;
            let post_count = ((counts >> 7) & 0x7) as usize;
            let succ_count = ((counts >> 1) & 0x3F) as usize;

            let payload = &bytes[8..];
            let mut offset = 0;

            let pre_effects = read_u16_vec(payload, &mut offset, pre_count)
                .into_iter()
                .map(|v| EffectOp::from_bytes(v.to_le_bytes()))
                .collect();
            let neg_fields = read_u16_vec(payload, &mut offset, neg_count);
            let post_effects = read_u16_vec(payload, &mut offset, post_count)
                .into_iter()
                .map(|v| EffectOp::from_bytes(v.to_le_bytes()))
                .collect();
            let successors = read_u16_vec(payload, &mut offset, succ_count)
                .into_iter()
                .map(StepId::new)
                .collect();

            Self {
                segment,
                nav,
                node_type,
                node_field,
                pre_effects,
                neg_fields,
                post_effects,
                successors,
            }
        }
    }

    /// Encode to bytecode bytes.
    ///
    /// Automatically selects the smallest opcode that fits the payload.
    /// Returns None if the payload is too large (> 28 u16 slots).
    pub fn to_bytes(&self) -> Option<Vec<u8>> {
        // Match8 can be used if: no effects, no neg_fields, and at most 1 successor
        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.successors.len() <= 1;

        let opcode = if can_use_match8 {
            Opcode::Match8
        } else {
            // Extended match: count all payload slots
            let slots_needed = self.pre_effects.len()
                + self.neg_fields.len()
                + self.post_effects.len()
                + self.successors.len();
            select_match_opcode(slots_needed)?
        };
        let size = opcode.size();
        let mut bytes = vec![0u8; size];

        // Type ID byte
        bytes[0] = (self.segment << 4) | (opcode as u8);
        bytes[1] = self.nav.to_byte();

        // Node type/field
        let node_type_val = self.node_type.map(|n| n.get()).unwrap_or(0);
        bytes[2..4].copy_from_slice(&node_type_val.to_le_bytes());
        let node_field_val = self.node_field.map(|n| n.get()).unwrap_or(0);
        bytes[4..6].copy_from_slice(&node_field_val.to_le_bytes());

        if opcode == Opcode::Match8 {
            // Match8: single successor or terminal (0)
            let next = self.successors.first().map(|s| s.get()).unwrap_or(0);
            bytes[6..8].copy_from_slice(&next.to_le_bytes());
        } else {
            // Extended match: pack counts and payload
            let pre_count = self.pre_effects.len() as u16;
            let neg_count = self.neg_fields.len() as u16;
            let post_count = self.post_effects.len() as u16;
            let succ_count = self.successors.len() as u16;

            let counts =
                (pre_count << 13) | (neg_count << 10) | (post_count << 7) | (succ_count << 1);
            bytes[6..8].copy_from_slice(&counts.to_le_bytes());

            let mut offset = 8;

            // Write pre_effects
            for effect in &self.pre_effects {
                bytes[offset..offset + 2].copy_from_slice(&effect.to_bytes());
                offset += 2;
            }

            // Write neg_fields
            for &field in &self.neg_fields {
                bytes[offset..offset + 2].copy_from_slice(&field.to_le_bytes());
                offset += 2;
            }

            // Write post_effects
            for effect in &self.post_effects {
                bytes[offset..offset + 2].copy_from_slice(&effect.to_bytes());
                offset += 2;
            }

            // Write successors
            for succ in &self.successors {
                bytes[offset..offset + 2].copy_from_slice(&succ.get().to_le_bytes());
                offset += 2;
            }

            // Remaining bytes are already zero (padding)
        }

        Some(bytes)
    }
}

/// Zero-copy view into a Match instruction for efficient VM execution.
///
/// Unlike `Match`, this doesn't allocate - it stores a reference to the
/// bytecode and provides iterator methods for accessing effects and successors.
#[derive(Clone, Copy, Debug)]
pub struct MatchView<'a> {
    bytes: &'a [u8],
    /// Segment index (0-15, currently only 0 is used).
    pub segment: u8,
    /// Navigation command.
    pub nav: Nav,
    /// Node type constraint (None = wildcard).
    pub node_type: Option<NonZeroU16>,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NonZeroU16>,
    /// Whether this is Match8 (no payload) or extended.
    is_match8: bool,
    /// For Match8: the single successor (0 = terminal).
    match8_next: u16,
    /// For extended: counts packed into single byte each.
    pre_count: u8,
    neg_count: u8,
    post_count: u8,
    succ_count: u8,
}

impl<'a> MatchView<'a> {
    /// Parse a Match instruction from bytecode without allocating.
    ///
    /// The slice must start at the instruction and contain at least
    /// the full instruction size (determined by opcode).
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        debug_assert!(bytes.len() >= 8, "Match instruction too short");

        let type_id_byte = bytes[0];
        let segment = type_id_byte >> 4;
        debug_assert!(segment == 0, "non-zero segment not yet supported");
        let opcode = Opcode::from_u8(type_id_byte & 0xF);
        debug_assert!(opcode.is_match(), "expected Match opcode");

        let nav = Nav::from_byte(bytes[1]);
        let node_type = NonZeroU16::new(u16::from_le_bytes([bytes[2], bytes[3]]));
        let node_field = NonZeroU16::new(u16::from_le_bytes([bytes[4], bytes[5]]));

        if opcode == Opcode::Match8 {
            let next = u16::from_le_bytes([bytes[6], bytes[7]]);
            Self {
                bytes,
                segment,
                nav,
                node_type,
                node_field,
                is_match8: true,
                match8_next: next,
                pre_count: 0,
                neg_count: 0,
                post_count: 0,
                succ_count: if next == 0 { 0 } else { 1 },
            }
        } else {
            let counts = u16::from_le_bytes([bytes[6], bytes[7]]);
            Self {
                bytes,
                segment,
                nav,
                node_type,
                node_field,
                is_match8: false,
                match8_next: 0,
                pre_count: ((counts >> 13) & 0x7) as u8,
                neg_count: ((counts >> 10) & 0x7) as u8,
                post_count: ((counts >> 7) & 0x7) as u8,
                succ_count: ((counts >> 1) & 0x3F) as u8,
            }
        }
    }

    /// Check if this is a terminal (accept) state.
    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.succ_count == 0
    }

    /// Check if this is an epsilon transition (no node interaction).
    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Stay && self.node_type.is_none() && self.node_field.is_none()
    }

    /// Number of successors.
    #[inline]
    pub fn succ_count(&self) -> usize {
        self.succ_count as usize
    }

    /// Get a successor by index.
    #[inline]
    pub fn successor(&self, idx: usize) -> StepId {
        debug_assert!(
            idx < self.succ_count as usize,
            "successor index out of bounds"
        );
        if self.is_match8 {
            debug_assert!(idx == 0);
            // Safe: we only call this when succ_count > 0, meaning match8_next != 0
            StepId(NonZeroU16::new(self.match8_next).unwrap())
        } else {
            let offset = self.succ_offset() + idx * 2;
            StepId::new(u16::from_le_bytes([
                self.bytes[offset],
                self.bytes[offset + 1],
            ]))
        }
    }

    /// Iterate over pre-effects (executed before match attempt).
    #[inline]
    pub fn pre_effects(&self) -> impl Iterator<Item = EffectOp> + '_ {
        let start = 8; // payload starts at byte 8
        (0..self.pre_count as usize).map(move |i| {
            let offset = start + i * 2;
            EffectOp::from_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    /// Iterate over negated fields (must NOT be present on matched node).
    #[inline]
    pub fn neg_fields(&self) -> impl Iterator<Item = u16> + '_ {
        let start = 8 + (self.pre_count as usize) * 2;
        (0..self.neg_count as usize).map(move |i| {
            let offset = start + i * 2;
            u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    /// Iterate over post-effects (executed after successful match).
    #[inline]
    pub fn post_effects(&self) -> impl Iterator<Item = EffectOp> + '_ {
        let start = 8 + (self.pre_count as usize + self.neg_count as usize) * 2;
        (0..self.post_count as usize).map(move |i| {
            let offset = start + i * 2;
            EffectOp::from_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    /// Iterate over successors.
    #[inline]
    pub fn successors(&self) -> impl Iterator<Item = StepId> + '_ {
        (0..self.succ_count as usize).map(move |i| self.successor(i))
    }

    /// Byte offset where successors start in the payload.
    #[inline]
    fn succ_offset(&self) -> usize {
        8 + (self.pre_count as usize + self.neg_count as usize + self.post_count as usize) * 2
    }
}

/// Call instruction for invoking definitions (recursion).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    /// Segment index (0-15).
    pub segment: u8,
    /// Navigation to apply before jumping to target.
    pub nav: Nav,
    /// Field constraint (None = no constraint).
    pub node_field: Option<NonZeroU16>,
    /// Return address (current segment).
    pub next: StepId,
    /// Callee entry point (target segment from type_id).
    pub target: StepId,
}

impl Call {
    /// Decode from 8-byte bytecode.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let type_id_byte = bytes[0];
        let segment = type_id_byte >> 4;
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        let opcode = Opcode::from_u8(type_id_byte & 0xF);
        assert_eq!(opcode, Opcode::Call, "expected Call opcode");

        Self {
            segment,
            nav: Nav::from_byte(bytes[1]),
            node_field: NonZeroU16::new(u16::from_le_bytes([bytes[2], bytes[3]])),
            next: StepId::new(u16::from_le_bytes([bytes[4], bytes[5]])),
            target: StepId::new(u16::from_le_bytes([bytes[6], bytes[7]])),
        }
    }

    /// Encode to 8-byte bytecode.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = (self.segment << 4) | (Opcode::Call as u8);
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&self.node_field.map_or(0, |v| v.get()).to_le_bytes());
        bytes[4..6].copy_from_slice(&self.next.get().to_le_bytes());
        bytes[6..8].copy_from_slice(&self.target.get().to_le_bytes());
        bytes
    }
}

/// Return instruction for returning from definitions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Return {
    /// Segment index (0-15).
    pub segment: u8,
}

impl Return {
    /// Decode from 8-byte bytecode.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let type_id_byte = bytes[0];
        let segment = type_id_byte >> 4;
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        let opcode = Opcode::from_u8(type_id_byte & 0xF);
        assert_eq!(opcode, Opcode::Return, "expected Return opcode");

        Self { segment }
    }

    /// Encode to 8-byte bytecode.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = (self.segment << 4) | (Opcode::Return as u8);
        // bytes[1..8] are reserved/padding
        bytes
    }
}

/// Select the smallest Match variant that fits the given payload.
pub fn select_match_opcode(slots_needed: usize) -> Option<Opcode> {
    if slots_needed == 0 {
        return Some(Opcode::Match8);
    }
    match slots_needed {
        1..=4 => Some(Opcode::Match16),
        5..=8 => Some(Opcode::Match24),
        9..=12 => Some(Opcode::Match32),
        13..=20 => Some(Opcode::Match48),
        21..=28 => Some(Opcode::Match64),
        _ => None, // Too large, must split
    }
}

/// Pad a size to the next multiple of SECTION_ALIGN (64 bytes).
#[inline]
pub fn align_to_section(size: usize) -> usize {
    (size + SECTION_ALIGN - 1) & !(SECTION_ALIGN - 1)
}
