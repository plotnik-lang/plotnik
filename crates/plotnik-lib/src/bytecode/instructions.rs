//! Bytecode instruction definitions.
//!
//! Instructions are runtime-friendly structs with `from_bytes`/`to_bytes`
//! methods for bytecode serialization.

use std::num::NonZeroU16;

use super::constants::{SECTION_ALIGN, STEP_SIZE};
use super::effects::EffectOp;
use super::nav::Nav;

/// Step address in bytecode (raw u16).
///
/// Used for layout addresses, entrypoint targets, bootstrap parameter, etc.
/// For decoded instruction successors (where 0 = terminal), use [`StepId`] instead.
pub type StepAddr = u16;

/// Successor step address in decoded instructions.
///
/// Uses NonZeroU16 because raw 0 means "terminal" (no successor).
/// This type is only for decoded instruction successors - use raw `u16`
/// for addresses in layout, entrypoints, and VM internals.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct StepId(pub NonZeroU16);

impl StepId {
    /// Create a new StepId. Panics if n == 0.
    #[inline]
    pub fn new(n: u16) -> Self {
        Self(NonZeroU16::new(n).expect("StepId cannot be 0"))
    }

    /// Get the raw u16 value.
    #[inline]
    pub fn get(self) -> u16 {
        self.0.get()
    }
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
    Trampoline = 0x8,
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
            0x8 => Self::Trampoline,
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
            Self::Trampoline => 8,
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

/// Match instruction decoded from bytecode.
///
/// Provides iterator-based access to effects and successors without allocating.
#[derive(Clone, Copy, Debug)]
pub struct Match<'a> {
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

impl<'a> Match<'a> {
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

        let (is_match8, match8_next, pre_count, neg_count, post_count, succ_count) =
            if opcode == Opcode::Match8 {
                let next = u16::from_le_bytes([bytes[6], bytes[7]]);
                (true, next, 0, 0, 0, if next == 0 { 0 } else { 1 })
            } else {
                let counts = u16::from_le_bytes([bytes[6], bytes[7]]);
                (
                    false,
                    0,
                    ((counts >> 13) & 0x7) as u8,
                    ((counts >> 10) & 0x7) as u8,
                    ((counts >> 7) & 0x7) as u8,
                    ((counts >> 1) & 0x3F) as u8,
                )
            };

        Self {
            bytes,
            segment,
            nav,
            node_type,
            node_field,
            is_match8,
            match8_next,
            pre_count,
            neg_count,
            post_count,
            succ_count,
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
            debug_assert!(self.match8_next != 0, "terminal has no successors");
            StepId::new(self.match8_next)
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

/// Trampoline instruction for universal entry.
///
/// Like Call, but the target comes from VM context (external parameter)
/// rather than being encoded in the instruction. Used at address 0 for
/// the entry preamble: `Obj → Trampoline → EndObj → Accept`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trampoline {
    /// Segment index (0-15).
    pub segment: u8,
    /// Return address (where to continue after entrypoint returns).
    pub next: StepId,
}

impl Trampoline {
    /// Decode from 8-byte bytecode.
    pub fn from_bytes(bytes: [u8; 8]) -> Self {
        let type_id_byte = bytes[0];
        let segment = type_id_byte >> 4;
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        let opcode = Opcode::from_u8(type_id_byte & 0xF);
        assert_eq!(opcode, Opcode::Trampoline, "expected Trampoline opcode");

        Self {
            segment,
            next: StepId::new(u16::from_le_bytes([bytes[2], bytes[3]])),
        }
    }

    /// Encode to 8-byte bytecode.
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = (self.segment << 4) | (Opcode::Trampoline as u8);
        // bytes[1] is padding
        bytes[2..4].copy_from_slice(&self.next.get().to_le_bytes());
        // bytes[4..8] are reserved/padding
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
