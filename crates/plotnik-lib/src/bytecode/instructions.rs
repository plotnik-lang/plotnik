//! Bytecode instruction definitions.
//!
//! Instructions are runtime-friendly structs with `from_bytes`/`to_bytes`
//! methods for bytecode serialization.

use std::num::NonZeroU16;

use crate::core::{NodeFieldId, ZeroIdError};

use super::constants::{
    BYTECODE_WORD_SIZE, MAX_EFFECTS, MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS, MAX_SUCCESSORS,
};
use super::effects::{EFFECT_PAYLOAD_MAX, Effect};
use super::node_kind_constraint::NodeKindConstraint;
use plotnik_rt::{Nav, PortId};

/// Fixed header bytes before an extended Match's payload — exactly the first
/// bytecode word. Effects, negated fields, an optional predicate, and successors follow,
/// each occupying [`PAYLOAD_SLOT_SIZE`] bytes.
pub(crate) const MATCH_PAYLOAD_START: usize = BYTECODE_WORD_SIZE;

/// Each Match payload slot is one little-endian `u16`.
pub(crate) const PAYLOAD_SLOT_SIZE: usize = size_of::<u16>();

/// A predicate occupies two payload slots: `op|flags (u16)`, then `value_ref (u16)`.
pub(crate) const PREDICATE_SLOTS: usize = 2;

/// A predicate's size in bytes within the payload.
pub(crate) const PREDICATE_SIZE: usize = PREDICATE_SLOTS * PAYLOAD_SLOT_SIZE;

/// The first byte of every instruction: `segment(2) | node_class(2) | opcode(4)`.
///
/// One source of truth for the header-byte field positions, shared by every
/// instruction decoder/encoder and the load-time validator.
pub(crate) mod header_byte {
    use super::Opcode;

    const OPCODE_MASK: u8 = 0x0F;
    const FIELD2_MASK: u8 = 0b11;
    const NODE_CLASS_SHIFT: u32 = 4;
    const SEGMENT_SHIFT: u32 = 6;

    /// The raw opcode nibble (low 4 bits), without validating it.
    pub(crate) fn opcode_nibble(b: u8) -> u8 {
        b & OPCODE_MASK
    }

    /// The opcode, or `None` if the nibble is unassigned (untrusted input).
    pub(crate) fn opcode(b: u8) -> Option<Opcode> {
        Opcode::from_u8(opcode_nibble(b))
    }

    /// The 2-bit segment field (bits 7-6).
    pub(crate) fn segment(b: u8) -> u8 {
        (b >> SEGMENT_SHIFT) & FIELD2_MASK
    }

    /// The 2-bit opcode-specific field (bits 5-4): node class for Match,
    /// ownership/contract flags for Call and Return.
    pub(crate) fn node_class_bits(b: u8) -> u8 {
        (b >> NODE_CLASS_SHIFT) & FIELD2_MASK
    }

    /// Assemble a header byte from its fields.
    pub(crate) fn pack(segment: u8, node_class: u8, opcode: Opcode) -> u8 {
        (segment << SEGMENT_SHIFT) | (node_class << NODE_CLASS_SHIFT) | (opcode as u8)
    }
}

/// The 16-bit counts word of an extended Match (`Match16`–`Match64`):
/// `effects(4) | neg(3) | succ(5) | has_predicate(1) | missing(1) | reserved(2)`.
///
/// `missing` and `has_predicate` are independent flags on adjacent bits, not a
/// 2-bit enum: `missing` (the `(MISSING)` node-nature constraint, checked by the
/// VM) is orthogonal to `has_predicate` (a payload-framing bit the decoder needs
/// to size the instruction), so they compose freely.
///
/// Decoded once here so the Match decoder, the encoder, and the load-time
/// validator share one definition of the field positions.
#[derive(Clone, Copy)]
pub(crate) struct MatchCounts {
    pub(crate) effects: u8,
    pub(crate) neg: u8,
    pub(crate) succ: u8,
    pub(crate) has_predicate: bool,
    pub(crate) missing: bool,
}

impl MatchCounts {
    const EFFECTS_SHIFT: u32 = 12;
    const NEG_SHIFT: u32 = 9;
    const SUCC_SHIFT: u32 = 4;
    const EFFECTS_MASK: u16 = 0xF;
    const COUNT3_MASK: u16 = 0x7;
    const SUCC_MASK: u16 = 0x1F;
    const PREDICATE_BIT: u16 = 1 << 3;
    const MISSING_BIT: u16 = 1 << 2;
    const RESERVED_MASK: u16 = 0x3;

    pub(crate) fn unpack(w: u16) -> Self {
        Self {
            effects: ((w >> Self::EFFECTS_SHIFT) & Self::EFFECTS_MASK) as u8,
            neg: ((w >> Self::NEG_SHIFT) & Self::COUNT3_MASK) as u8,
            succ: ((w >> Self::SUCC_SHIFT) & Self::SUCC_MASK) as u8,
            has_predicate: w & Self::PREDICATE_BIT != 0,
            missing: w & Self::MISSING_BIT != 0,
        }
    }

    pub(crate) fn pack(self) -> u16 {
        ((self.effects as u16) << Self::EFFECTS_SHIFT)
            | ((self.neg as u16) << Self::NEG_SHIFT)
            | ((self.succ as u16) << Self::SUCC_SHIFT)
            | if self.has_predicate {
                Self::PREDICATE_BIT
            } else {
                0
            }
            | if self.missing { Self::MISSING_BIT } else { 0 }
    }

    /// Whether any reserved bit (bits 1-0) is set; load-time validation rejects it.
    pub(crate) fn reserved_bits_set(w: u16) -> bool {
        w & Self::RESERVED_MASK != 0
    }
}

/// Bytecode-word address in the instruction stream.
///
/// Used for layout addresses, entry point targets, bootstrap parameter, etc.
/// For decoded instruction successors (where 0 = terminal), use [`SuccessorAddr`] instead.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct CodeAddr(u16);

impl CodeAddr {
    pub const ZERO: Self = Self(0);

    #[inline]
    pub const fn get(self) -> u16 {
        self.0
    }

    #[inline]
    pub const fn as_usize(self) -> usize {
        self.0 as usize
    }

    #[inline]
    pub const fn checked_add(self, words: u16) -> Option<Self> {
        match self.0.checked_add(words) {
            Some(addr) => Some(Self(addr)),
            None => None,
        }
    }

    #[inline]
    pub fn to_le_bytes(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }
}

impl From<u16> for CodeAddr {
    #[inline]
    fn from(n: u16) -> Self {
        Self(n)
    }
}

impl From<CodeAddr> for u16 {
    #[inline]
    fn from(v: CodeAddr) -> Self {
        v.0
    }
}

impl std::fmt::Display for CodeAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Encoded non-terminal successor address in decoded instructions.
///
/// Uses NonZeroU16 because raw 0 means "terminal" (no successor).
/// Use [`CodeAddr`] where zero is a valid address rather than a terminal marker.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct SuccessorAddr(NonZeroU16);

impl From<NonZeroU16> for SuccessorAddr {
    #[inline]
    fn from(n: NonZeroU16) -> Self {
        Self(n)
    }
}
impl From<SuccessorAddr> for NonZeroU16 {
    #[inline]
    fn from(v: SuccessorAddr) -> Self {
        v.0
    }
}
impl From<SuccessorAddr> for u16 {
    #[inline]
    fn from(v: SuccessorAddr) -> Self {
        v.0.get()
    }
}
impl TryFrom<u16> for SuccessorAddr {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
    }
}

impl TryFrom<CodeAddr> for SuccessorAddr {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(addr: CodeAddr) -> Result<Self, Self::Error> {
        Self::try_from(u16::from(addr))
    }
}
impl std::fmt::Display for SuccessorAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.get())
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
    Call1 = 0x6,
    CallN = 0x7,
    Return = 0x8,
}

impl Opcode {
    /// Decode an opcode nibble, returning `None` for an unknown value so that
    /// malformed bytecode is rejected with a clean error instead of panicking.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(Self::Match8),
            0x1 => Some(Self::Match16),
            0x2 => Some(Self::Match24),
            0x3 => Some(Self::Match32),
            0x4 => Some(Self::Match48),
            0x5 => Some(Self::Match64),
            0x6 => Some(Self::Call1),
            0x7 => Some(Self::CallN),
            0x8 => Some(Self::Return),
            _ => None,
        }
    }

    /// Instruction size in bytes. The variant ladder is defined here; payload
    /// capacity and word count derive from it.
    pub const fn size(self) -> usize {
        match self {
            Self::Match8 => 8,
            Self::Match16 => 16,
            Self::Match24 => 24,
            Self::Match32 => 32,
            Self::Match48 => 48,
            Self::Match64 => 64,
            Self::Call1 | Self::Return => BYTECODE_WORD_SIZE,
            Self::CallN => 3 * BYTECODE_WORD_SIZE,
        }
    }

    /// Number of bytecode words this instruction occupies.
    pub const fn word_count(self) -> u16 {
        (self.size() / BYTECODE_WORD_SIZE) as u16
    }

    pub const fn is_match(self) -> bool {
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

    /// Payload capacity in u16 slots — whatever follows the one-word header.
    /// Zero for non-extended variants (Match8, Call, Return).
    pub const fn payload_slots(self) -> usize {
        if !self.is_match() || matches!(self, Self::Match8) {
            return 0;
        }
        (self.size() - MATCH_PAYLOAD_START) / PAYLOAD_SLOT_SIZE
    }
}

/// `MAX_MATCH_PAYLOAD_SLOTS` must track the largest variant's capacity.
const _: () = assert!(MAX_MATCH_PAYLOAD_SLOTS == Opcode::Match64.payload_slots());

/// Match instruction decoded from bytecode.
///
/// Provides iterator-based access to effects and successors without allocating.
#[derive(Clone, Copy, Debug)]
pub struct Match<'a> {
    bytes: &'a [u8],
    /// Segment index (0-3, currently only 0 is used).
    pub segment: u8,
    /// Navigation command. `Epsilon` means no cursor movement or node check.
    pub nav: Nav,
    /// Node kind constraint (Any = wildcard, Named/Anonymous for specific checks).
    pub node_kind: NodeKindConstraint,
    /// Field constraint (None = wildcard).
    pub node_field: Option<NodeFieldId>,
    layout: MatchLayout,
}

/// The two payload shapes of a [`Match`], discriminated by the opcode: the
/// `Match8` fast path carries a single inline successor, while `Extended`
/// (`Match16`-`Match64`) packs the counts addressing the effect slots,
/// successors, and optional predicate.
#[derive(Clone, Copy, Debug)]
enum MatchLayout {
    /// `next == 0` means terminal.
    Match8 { next: u16 },
    Extended {
        effect_count: u8,
        neg_count: u8,
        succ_count: u8,
        has_predicate: bool,
        missing: bool,
    },
}

impl<'a> Match<'a> {
    /// Parse a Match instruction from bytecode without allocating.
    ///
    /// The slice must start at the instruction and contain at least
    /// the full instruction size (determined by opcode).
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    #[inline]
    pub fn from_bytes(bytes: &'a [u8]) -> Self {
        debug_assert!(
            bytes.len() >= BYTECODE_WORD_SIZE,
            "Match instruction too short"
        );

        let header = bytes[0];
        let segment = header_byte::segment(header);
        let node_class = header_byte::node_class_bits(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        debug_assert!(segment == 0, "non-zero segment not yet supported");
        debug_assert!(opcode.is_match(), "expected Match opcode");

        let nav = Nav::from_byte(bytes[1]);
        let node_val = u16::from_le_bytes([bytes[2], bytes[3]]);
        let node_kind = NodeKindConstraint::from_bytes(node_class, node_val);
        let node_field =
            NonZeroU16::new(u16::from_le_bytes([bytes[4], bytes[5]])).map(NodeFieldId::from);

        let layout = if opcode == Opcode::Match8 {
            let next = u16::from_le_bytes([bytes[6], bytes[7]]);
            MatchLayout::Match8 { next }
        } else {
            let c = MatchCounts::unpack(u16::from_le_bytes([bytes[6], bytes[7]]));
            MatchLayout::Extended {
                effect_count: c.effects,
                neg_count: c.neg,
                succ_count: c.succ,
                has_predicate: c.has_predicate,
                missing: c.missing,
            }
        };

        Self {
            bytes,
            segment,
            nav,
            node_kind,
            node_field,
            layout,
        }
    }

    #[inline]
    pub fn is_terminal(&self) -> bool {
        self.succ_count() == 0
    }

    #[inline]
    pub fn is_epsilon(&self) -> bool {
        self.nav == Nav::Epsilon
    }

    /// Check if this is a Match8 (8-byte fast-path instruction).
    #[inline]
    pub fn is_match8(&self) -> bool {
        matches!(self.layout, MatchLayout::Match8 { .. })
    }

    /// Bytecode words this instruction occupies, read from its opcode.
    #[inline]
    pub fn word_count(&self) -> u16 {
        header_byte::opcode(self.bytes[0])
            .expect("decoded Match has a valid opcode")
            .word_count()
    }

    /// Number of successors.
    #[inline]
    pub fn succ_count(&self) -> usize {
        match self.layout {
            MatchLayout::Match8 { next } => (next != 0) as usize,
            MatchLayout::Extended { succ_count, .. } => succ_count as usize,
        }
    }

    #[inline]
    pub fn successor(&self, idx: usize) -> SuccessorAddr {
        debug_assert!(idx < self.succ_count(), "successor index out of bounds");
        match self.layout {
            MatchLayout::Match8 { next } => {
                debug_assert!(idx == 0);
                debug_assert!(next != 0, "terminal has no successors");
                SuccessorAddr::try_from(next).expect("successor address must be non-zero")
            }
            MatchLayout::Extended { .. } => {
                let offset = self.succ_offset() + idx * PAYLOAD_SLOT_SIZE;
                SuccessorAddr::try_from(u16::from_le_bytes([
                    self.bytes[offset],
                    self.bytes[offset + 1],
                ]))
                .expect("successor address must be non-zero")
            }
        }
    }

    /// Iterate over effects executed after match acceptance, in bytecode order.
    #[inline]
    pub fn effects(&self) -> impl Iterator<Item = Effect> + '_ {
        let start = MATCH_PAYLOAD_START;
        (0..self.effect_count()).map(move |i| {
            let offset = start + i * PAYLOAD_SLOT_SIZE;
            Effect::from_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    /// Iterate over negated fields (must NOT be present on matched node).
    #[inline]
    pub fn neg_fields(&self) -> impl Iterator<Item = NodeFieldId> + '_ {
        let start = MATCH_PAYLOAD_START + self.effect_count() * PAYLOAD_SLOT_SIZE;
        (0..self.neg_count()).map(move |i| {
            let offset = start + i * PAYLOAD_SLOT_SIZE;
            let raw = u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
            NodeFieldId::try_from(raw).expect("neg field id must be non-zero")
        })
    }

    #[inline]
    pub fn successors(&self) -> impl Iterator<Item = SuccessorAddr> + '_ {
        (0..self.succ_count()).map(move |i| self.successor(i))
    }

    #[inline]
    pub fn has_predicate(&self) -> bool {
        matches!(
            self.layout,
            MatchLayout::Extended {
                has_predicate: true,
                ..
            }
        )
    }

    /// Whether the matched node must be a tree-sitter MISSING node (a zero-byte
    /// node the parser inserts during error recovery). Only extended Matches can
    /// carry it — the `Match8` fast path has no counts word.
    #[inline]
    pub fn missing(&self) -> bool {
        matches!(self.layout, MatchLayout::Extended { missing: true, .. })
    }

    /// Get predicate data if present.
    pub fn predicate(&self) -> Option<MatchPredicate> {
        if !self.has_predicate() {
            return None;
        }

        let offset = MATCH_PAYLOAD_START + self.predicate_offset();
        let op_and_flags = u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
        let (op, is_regex) = MatchPredicate::unpack_op_flags(op_and_flags);
        let value_ref = u16::from_le_bytes([
            self.bytes[offset + PAYLOAD_SLOT_SIZE],
            self.bytes[offset + PAYLOAD_SLOT_SIZE + 1],
        ]);

        Some(MatchPredicate {
            op,
            is_regex,
            value_ref,
        })
    }

    #[inline]
    fn effect_count(&self) -> usize {
        match self.layout {
            MatchLayout::Extended { effect_count, .. } => effect_count as usize,
            MatchLayout::Match8 { .. } => 0,
        }
    }

    #[inline]
    fn neg_count(&self) -> usize {
        match self.layout {
            MatchLayout::Extended { neg_count, .. } => neg_count as usize,
            MatchLayout::Match8 { .. } => 0,
        }
    }

    /// Byte offset where an optional predicate starts in the payload.
    #[inline]
    fn predicate_offset(&self) -> usize {
        (self.effect_count() + self.neg_count()) * PAYLOAD_SLOT_SIZE
    }

    /// Byte offset where successors start in the payload.
    /// Accounts for the predicate slots if present.
    #[inline]
    fn succ_offset(&self) -> usize {
        let predicate_size = if self.has_predicate() {
            PREDICATE_SIZE
        } else {
            0
        };
        MATCH_PAYLOAD_START + self.predicate_offset() + predicate_size
    }

    /// Collect this borrowed view into an owned, encodable [`MatchInstr`].
    ///
    /// Lets the decoder be re-encoded for roundtrip testing.
    pub fn to_instr(self) -> MatchInstr {
        MatchInstr {
            nav: self.nav,
            node_kind: self.node_kind,
            node_field: self.node_field,
            missing: self.missing(),
            effects: self.effects().collect(),
            neg_fields: self.neg_fields().collect(),
            predicate: self.predicate(),
            successors: self.successors().collect(),
        }
    }
}

/// Predicate filter carried by an extended Match (text comparison).
///
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatchPredicate {
    /// Operator byte (see [`crate::bytecode::predicate_op::PredicateOp`]).
    pub op: u8,
    /// Whether `value_ref` indexes the regex table (`true`) or string table.
    pub is_regex: bool,
    /// Index into the string or regex table.
    pub value_ref: u16,
}

impl MatchPredicate {
    /// Operator occupies the low byte of the op/flags word.
    const OP_MASK: u16 = 0xFF;
    /// Bit 8 flags a regex operand; the operator byte is below it.
    const REGEX_FLAG: u16 = 1 << 8;
    /// Bits above the operator and regex flag are reserved-zero.
    const RESERVED_MASK: u16 = !(Self::OP_MASK | Self::REGEX_FLAG);

    /// Pack the op/flags word — the predicate's first payload slot.
    fn pack_op_flags(self) -> u16 {
        (self.op as u16) | if self.is_regex { Self::REGEX_FLAG } else { 0 }
    }

    /// Unpack `(op, is_regex)` from the op/flags word. Shared by the decoder and
    /// the load-time validator.
    pub(crate) fn unpack_op_flags(w: u16) -> (u8, bool) {
        ((w & Self::OP_MASK) as u8, w & Self::REGEX_FLAG != 0)
    }

    /// Whether any reserved bit of the op/flags word is set; load-time
    /// validation rejects it.
    pub(crate) fn reserved_bits_set(w: u16) -> bool {
        w & Self::RESERVED_MASK != 0
    }
}

/// Owned, encodable form of a Match instruction.
///
/// This is the encode-side mirror of the [`Match`] decoder: the emitter
/// resolves symbolic references into one of these and calls [`encode`](Self::encode),
/// keeping encode and decode in one crate so a roundtrip can be property-tested.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct MatchInstr {
    pub nav: Nav,
    pub node_kind: NodeKindConstraint,
    pub node_field: Option<NodeFieldId>,
    /// Node must be a tree-sitter MISSING node — the `(MISSING …)` constraint.
    pub missing: bool,
    pub effects: Vec<Effect>,
    pub neg_fields: Vec<NodeFieldId>,
    pub predicate: Option<MatchPredicate>,
    pub successors: Vec<SuccessorAddr>,
}

/// Error returned when an instruction cannot be encoded into bytecode.
///
/// Every variant is reachable from a `check`-clean query, so the emitter
/// surfaces these as compile errors instead of letting an `as`-narrowing wrap
/// or an `assert!` panic at encode time.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("too many effects on one match: {0} (max {MAX_EFFECTS})")]
    TooManyEffects(usize),
    #[error("too many negated fields on one match: {0} (max {MAX_NEG_FIELDS})")]
    TooManyNegFields(usize),
    #[error("too many successors on one match: {0} (max {MAX_SUCCESSORS})")]
    TooManySuccessors(usize),
    #[error("match payload too large: {0} slots (max {MAX_MATCH_PAYLOAD_SLOTS})")]
    PayloadTooLarge(usize),
    #[error("effect payload exceeds limit: {0} (max {EFFECT_PAYLOAD_MAX})")]
    EffectPayloadOverflow(usize),
}

impl MatchInstr {
    /// Encode to bytecode bytes, choosing the smallest fitting Match variant.
    ///
    /// Returns [`EncodeError`] rather than panicking when a count or payload
    /// exceeds what the format can represent.
    pub fn encode(&self) -> Result<Vec<u8>, EncodeError> {
        for effect in &self.effects {
            if effect.payload > EFFECT_PAYLOAD_MAX {
                return Err(EncodeError::EffectPayloadOverflow(effect.payload));
            }
        }

        let (node_class, node_val) = self.node_kind.to_bytes();
        let node_field_val = self.node_field.map_or(0, u16::from);

        let can_use_match8 = self.effects.is_empty()
            && self.neg_fields.is_empty()
            && self.predicate.is_none()
            && !self.missing
            && self.successors.len() <= 1;

        if can_use_match8 {
            let mut bytes = vec![0u8; Opcode::Match8.size()];
            bytes[0] = header_byte::pack(0, node_class, Opcode::Match8);
            bytes[1] = self.nav.to_byte();
            bytes[2..4].copy_from_slice(&node_val.to_le_bytes());
            bytes[4..6].copy_from_slice(&node_field_val.to_le_bytes());
            let next = self.successors.first().map_or(0, |s| u16::from(*s));
            bytes[6..8].copy_from_slice(&next.to_le_bytes());
            return Ok(bytes);
        }

        let effects = self.effects.len();
        let neg = self.neg_fields.len();
        let succ = self.successors.len();
        if effects > MAX_EFFECTS {
            return Err(EncodeError::TooManyEffects(effects));
        }
        if neg > MAX_NEG_FIELDS {
            return Err(EncodeError::TooManyNegFields(neg));
        }
        if succ > MAX_SUCCESSORS {
            return Err(EncodeError::TooManySuccessors(succ));
        }

        let predicate_slots = if self.predicate.is_some() {
            PREDICATE_SLOTS
        } else {
            0
        };
        let slots = effects + neg + predicate_slots + succ;
        let opcode = match select_match_opcode(slots) {
            // A missing-only terminal has no payload slots but still needs the
            // counts word (for its `missing` bit), which the `Match8` fast path
            // lacks — floor it to the smallest extended variant.
            Some(Opcode::Match8) => Opcode::Match16,
            Some(op) => op,
            None => return Err(EncodeError::PayloadTooLarge(slots)),
        };

        let mut bytes = vec![0u8; opcode.size()];
        bytes[0] = header_byte::pack(0, node_class, opcode);
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&node_val.to_le_bytes());
        bytes[4..6].copy_from_slice(&node_field_val.to_le_bytes());

        let counts = MatchCounts {
            effects: effects as u8,
            neg: neg as u8,
            succ: succ as u8,
            has_predicate: self.predicate.is_some(),
            missing: self.missing,
        };
        bytes[6..8].copy_from_slice(&counts.pack().to_le_bytes());

        let mut offset = MATCH_PAYLOAD_START;
        let mut put = |bytes: &mut [u8], data: [u8; 2]| {
            bytes[offset..offset + PAYLOAD_SLOT_SIZE].copy_from_slice(&data);
            offset += PAYLOAD_SLOT_SIZE;
        };
        for effect in &self.effects {
            put(&mut bytes, effect.to_bytes());
        }
        for &field in &self.neg_fields {
            put(&mut bytes, u16::from(field).to_le_bytes());
        }
        if let Some(pred) = &self.predicate {
            put(&mut bytes, pred.pack_op_flags().to_le_bytes());
            put(&mut bytes, pred.value_ref.to_le_bytes());
        }
        for succ in &self.successors {
            put(&mut bytes, u16::from(*succ).to_le_bytes());
        }

        Ok(bytes)
    }
}

const CALL_MAX_RETURNS: usize = PortId::COUNT as usize;
const CALL_OWNERSHIP_BIT: u8 = 1;
const CALL1_CONSUMED_BIT: u8 = 2;

/// Which side of a call boundary owns navigation and field selection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CallOwnership {
    Caller,
    Callee,
}

impl CallOwnership {
    pub(crate) fn from_bit(bit: u8) -> Self {
        if bit == 0 { Self::Caller } else { Self::Callee }
    }

    pub(crate) fn to_bit(self) -> u8 {
        match self {
            Self::Caller => 0,
            Self::Callee => CALL_OWNERSHIP_BIT,
        }
    }
}

/// Immutable entry protocol implemented by a callee body.
///
/// Caller-owned bodies are exact and therefore carry no body-side navigation
/// or field. Callee-owned bodies embed the exact authored obligation that
/// every call targeting them must declare.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalleeContract {
    CallerOwned,
    CalleeOwned {
        nav: Nav,
        node_field: Option<NodeFieldId>,
    },
}

impl CalleeContract {
    pub fn ownership(self) -> CallOwnership {
        match self {
            Self::CallerOwned => CallOwnership::Caller,
            Self::CalleeOwned { .. } => CallOwnership::Callee,
        }
    }

    pub fn nav(self) -> Nav {
        match self {
            Self::CallerOwned => Nav::Stay,
            Self::CalleeOwned { nav, .. } => nav,
        }
    }

    pub fn node_field(self) -> Option<NodeFieldId> {
        match self {
            Self::CallerOwned => None,
            Self::CalleeOwned { node_field, .. } => node_field,
        }
    }
}

/// Unified decoded call view. `Call1` and `CallN` differ only in physical
/// density; execution and validation consume this semantic shape.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    pub segment: u8,
    pub ownership: CallOwnership,
    pub nav: Nav,
    pub node_field: Option<NodeFieldId>,
    pub target: SuccessorAddr,
    returns: [Option<SuccessorAddr>; CALL_MAX_RETURNS],
    returns_len: u8,
    consumed_mask: u8,
}

impl Call {
    pub fn new(
        ownership: CallOwnership,
        nav: Nav,
        node_field: Option<NodeFieldId>,
        return_addrs: &[SuccessorAddr],
        consumed_mask: u8,
        target: SuccessorAddr,
    ) -> Self {
        assert!(
            (1..=CALL_MAX_RETURNS).contains(&return_addrs.len()),
            "call must have 1..={CALL_MAX_RETURNS} return addresses"
        );
        let valid_mask = PortId::dense_mask(return_addrs.len());
        assert_eq!(
            consumed_mask & !valid_mask,
            0,
            "call consumed mask has bits outside its arity"
        );
        assert!(
            ownership == CallOwnership::Callee || consumed_mask == valid_mask,
            "caller-owned calls cannot expose empty ports"
        );

        let mut returns = [None; CALL_MAX_RETURNS];
        for (slot, &addr) in returns.iter_mut().zip(return_addrs) {
            *slot = Some(addr);
        }
        Self {
            segment: 0,
            ownership,
            nav,
            node_field,
            target,
            returns,
            returns_len: return_addrs.len() as u8,
            consumed_mask,
        }
    }

    pub fn arity(self) -> usize {
        usize::from(self.returns_len)
    }

    pub fn returns(&self) -> impl ExactSizeIterator<Item = SuccessorAddr> + '_ {
        self.returns[..self.arity()]
            .iter()
            .map(|addr| addr.expect("call return slots inside arity are populated"))
    }

    pub fn return_addr(self, port: PortId) -> SuccessorAddr {
        assert!(
            port.index() < self.arity(),
            "return port is within call arity"
        );
        self.returns[port.index()].expect("call return slots inside arity are populated")
    }

    pub fn port_consumed(self, port: PortId) -> bool {
        self.consumed_mask & port.bit() != 0
    }

    pub fn consumed_mask(self) -> u8 {
        self.consumed_mask
    }

    pub fn callee_contract(self) -> CalleeContract {
        match self.ownership {
            CallOwnership::Caller => CalleeContract::CallerOwned,
            CallOwnership::Callee => CalleeContract::CalleeOwned {
                nav: self.nav,
                node_field: self.node_field,
            },
        }
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        let header = bytes[0];
        let segment = header_byte::segment(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );

        let flags = header_byte::node_class_bits(header);
        let ownership = CallOwnership::from_bit(flags & CALL_OWNERSHIP_BIT);
        let nav = Nav::from_byte(bytes[1]);
        let node_field =
            NonZeroU16::new(u16::from_le_bytes([bytes[2], bytes[3]])).map(NodeFieldId::from);
        let successor_addr = |offset| {
            SuccessorAddr::try_from(u16::from_le_bytes([bytes[offset], bytes[offset + 1]]))
                .expect("successor address must be non-zero")
        };

        match opcode {
            Opcode::Call1 => {
                let consumed_mask = u8::from(flags & CALL1_CONSUMED_BIT != 0);
                Self::new(
                    ownership,
                    nav,
                    node_field,
                    &[successor_addr(4)],
                    consumed_mask,
                    successor_addr(6),
                )
            }
            Opcode::CallN => {
                let arity = usize::from(bytes[6]);
                let mut return_addrs = Vec::with_capacity(arity);
                for index in 0..arity {
                    return_addrs.push(successor_addr(BYTECODE_WORD_SIZE + index * 2));
                }
                Self::new(
                    ownership,
                    nav,
                    node_field,
                    &return_addrs,
                    bytes[7],
                    successor_addr(4),
                )
            }
            _ => unreachable!("Call::from_bytes requires Call1 or CallN"),
        }
    }

    pub fn to_bytes(self) -> Vec<u8> {
        if self.arity() == 1 {
            let mut bytes = vec![0u8; Opcode::Call1.size()];
            let flags = self.ownership.to_bit()
                | if self.consumed_mask & PortId::ZERO.bit() != 0 {
                    CALL1_CONSUMED_BIT
                } else {
                    0
                };
            bytes[0] = header_byte::pack(self.segment, flags, Opcode::Call1);
            bytes[1] = self.nav.to_byte();
            bytes[2..4].copy_from_slice(&self.node_field.map_or(0, u16::from).to_le_bytes());
            bytes[4..6].copy_from_slice(
                &u16::from(self.returns[0].expect("call has one return")).to_le_bytes(),
            );
            bytes[6..8].copy_from_slice(&u16::from(self.target).to_le_bytes());
            return bytes;
        }

        let mut bytes = vec![0u8; Opcode::CallN.size()];
        bytes[0] = header_byte::pack(self.segment, self.ownership.to_bit(), Opcode::CallN);
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&self.node_field.map_or(0, u16::from).to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::from(self.target).to_le_bytes());
        bytes[6] = self.returns_len;
        bytes[7] = self.consumed_mask;
        for (index, addr) in self.returns().enumerate() {
            let offset = BYTECODE_WORD_SIZE + index * 2;
            bytes[offset..offset + 2].copy_from_slice(&u16::from(addr).to_le_bytes());
        }
        bytes
    }
}

/// Return instruction for returning from definitions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Return {
    /// Segment index (0-3).
    pub segment: u8,
    pub port: PortId,
    pub contract: CalleeContract,
}

impl Return {
    pub fn new() -> Self {
        Self::port(PortId::ZERO)
    }

    pub fn port(port: PortId) -> Self {
        Self::with_contract(port, CalleeContract::CallerOwned)
    }

    pub fn with_contract(port: PortId, contract: CalleeContract) -> Self {
        Self {
            segment: 0,
            port,
            contract,
        }
    }

    /// Decode from 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    /// For Return, header flag bit 0 stores callee entry ownership.
    pub(crate) fn from_bytes(bytes: [u8; 8]) -> Self {
        let header = bytes[0];
        let segment = header_byte::segment(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        assert_eq!(opcode, Opcode::Return, "expected Return opcode");

        let flags = header_byte::node_class_bits(header);
        let ownership = CallOwnership::from_bit(flags & CALL_OWNERSHIP_BIT);
        let nav = Nav::from_byte(bytes[2]);
        let node_field =
            NonZeroU16::new(u16::from_le_bytes([bytes[4], bytes[5]])).map(NodeFieldId::from);
        let contract = match ownership {
            CallOwnership::Caller => CalleeContract::CallerOwned,
            CallOwnership::Callee => CalleeContract::CalleeOwned { nav, node_field },
        };

        Self {
            segment,
            port: PortId::from_byte(bytes[1]).expect("validated return port"),
            contract,
        }
    }

    /// Encode to 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    pub fn to_bytes(self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = header_byte::pack(
            self.segment,
            self.contract.ownership().to_bit(),
            Opcode::Return,
        );
        bytes[1] = self.port.to_byte();
        bytes[2] = self.contract.nav().to_byte();
        bytes[4..6].copy_from_slice(
            &self
                .contract
                .node_field()
                .map_or(0, u16::from)
                .to_le_bytes(),
        );
        // bytes 3 and 6-7 are reserved/padding.
        bytes
    }
}

impl Default for Return {
    fn default() -> Self {
        Self::new()
    }
}

/// Select the smallest Match variant that fits the given payload. Returns
/// `None` when no variant is large enough (the caller must split).
pub fn select_match_opcode(slots_needed: usize) -> Option<Opcode> {
    if slots_needed == 0 {
        return Some(Opcode::Match8);
    }
    [
        Opcode::Match16,
        Opcode::Match24,
        Opcode::Match32,
        Opcode::Match48,
        Opcode::Match64,
    ]
    .into_iter()
    .find(|op| op.payload_slots() >= slots_needed)
}
