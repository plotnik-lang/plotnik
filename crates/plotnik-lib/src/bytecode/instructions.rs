//! Bytecode instruction definitions.
//!
//! Instructions are runtime-friendly structs with `from_bytes`/`to_bytes`
//! methods for bytecode serialization.

use std::num::NonZeroU16;

use crate::core::{NodeFieldId, ZeroIdError};

#[cfg(test)]
use super::constants::SECTION_ALIGN;
use super::constants::{
    MAX_MATCH_PAYLOAD_SLOTS, MAX_NEG_FIELDS, MAX_POST_EFFECTS, MAX_PRE_EFFECTS, MAX_SUCCESSORS,
    STEP_SIZE,
};
use super::effects::{EFFECT_PAYLOAD_MAX, Effect};
use super::nav::Nav;
use super::node_kind_constraint::NodeKindConstraint;

/// Fixed header bytes before an extended Match's payload — exactly the first
/// step. Effects, negated fields, an optional predicate, and successors follow,
/// each occupying [`PAYLOAD_SLOT_SIZE`] bytes.
pub(crate) const MATCH_PAYLOAD_START: usize = STEP_SIZE;

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

    /// The 2-bit node-class field (bits 5-4); meaningful only for Match.
    pub(crate) fn node_class_bits(b: u8) -> u8 {
        (b >> NODE_CLASS_SHIFT) & FIELD2_MASK
    }

    /// Assemble a header byte from its fields.
    pub(crate) fn pack(segment: u8, node_class: u8, opcode: Opcode) -> u8 {
        (segment << SEGMENT_SHIFT) | (node_class << NODE_CLASS_SHIFT) | (opcode as u8)
    }
}

/// The 16-bit counts word of an extended Match (`Match16`–`Match64`):
/// `pre(3) | neg(3) | post(3) | succ(5) | has_predicate(1) | reserved(1)`.
///
/// Decoded once here so the Match decoder, the encoder, and the load-time
/// validator share one definition of the field positions.
#[derive(Clone, Copy)]
pub(crate) struct MatchCounts {
    pub(crate) pre: u8,
    pub(crate) neg: u8,
    pub(crate) post: u8,
    pub(crate) succ: u8,
    pub(crate) has_predicate: bool,
}

impl MatchCounts {
    const PRE_SHIFT: u32 = 13;
    const NEG_SHIFT: u32 = 10;
    const POST_SHIFT: u32 = 7;
    const SUCC_SHIFT: u32 = 2;
    const COUNT3_MASK: u16 = 0x7;
    const SUCC_MASK: u16 = 0x1F;
    const PREDICATE_BIT: u16 = 1 << 1;
    const RESERVED_MASK: u16 = 0x1;

    pub(crate) fn unpack(w: u16) -> Self {
        Self {
            pre: ((w >> Self::PRE_SHIFT) & Self::COUNT3_MASK) as u8,
            neg: ((w >> Self::NEG_SHIFT) & Self::COUNT3_MASK) as u8,
            post: ((w >> Self::POST_SHIFT) & Self::COUNT3_MASK) as u8,
            succ: ((w >> Self::SUCC_SHIFT) & Self::SUCC_MASK) as u8,
            has_predicate: w & Self::PREDICATE_BIT != 0,
        }
    }

    pub(crate) fn pack(self) -> u16 {
        ((self.pre as u16) << Self::PRE_SHIFT)
            | ((self.neg as u16) << Self::NEG_SHIFT)
            | ((self.post as u16) << Self::POST_SHIFT)
            | ((self.succ as u16) << Self::SUCC_SHIFT)
            | if self.has_predicate {
                Self::PREDICATE_BIT
            } else {
                0
            }
    }

    /// Whether the reserved bit (bit 0) is set; load-time validation rejects it.
    pub(crate) fn reserved_bit_set(w: u16) -> bool {
        w & Self::RESERVED_MASK != 0
    }
}

/// Step address in bytecode.
///
/// Used for layout addresses, entrypoint targets, bootstrap parameter, etc.
/// For decoded instruction successors (where 0 = terminal), use [`StepId`] instead.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct StepAddr(u16);

impl StepAddr {
    pub const PREAMBLE: Self = Self(0);

    #[inline]
    pub const fn get(self) -> u16 {
        self.0
    }

    #[inline]
    pub fn to_le_bytes(self) -> [u8; 2] {
        self.0.to_le_bytes()
    }
}

impl From<u16> for StepAddr {
    #[inline]
    fn from(n: u16) -> Self {
        Self(n)
    }
}

impl From<StepAddr> for u16 {
    #[inline]
    fn from(v: StepAddr) -> Self {
        v.0
    }
}

impl std::fmt::Display for StepAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

/// Successor step address in decoded instructions.
///
/// Uses NonZeroU16 because raw 0 means "terminal" (no successor).
/// This type is only for decoded instruction successors - use raw `u16`
/// for addresses in layout, entrypoints, and VM internals.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(transparent)]
pub struct StepId(NonZeroU16);

impl From<NonZeroU16> for StepId {
    #[inline]
    fn from(n: NonZeroU16) -> Self { Self(n) }
}
impl From<StepId> for NonZeroU16 {
    #[inline]
    fn from(v: StepId) -> Self { v.0 }
}
impl From<StepId> for u16 {
    #[inline]
    fn from(v: StepId) -> Self { v.0.get() }
}
impl TryFrom<u16> for StepId {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(n: u16) -> Result<Self, Self::Error> {
        NonZeroU16::new(n).map(Self).ok_or(ZeroIdError)
    }
}

impl TryFrom<StepAddr> for StepId {
    type Error = ZeroIdError;
    #[inline]
    fn try_from(addr: StepAddr) -> Result<Self, Self::Error> {
        Self::try_from(u16::from(addr))
    }
}
impl std::fmt::Display for StepId {
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
    Call = 0x6,
    Return = 0x7,
    Trampoline = 0x8,
}

impl Opcode {
    /// Decode an opcode nibble, returning `None` for an unknown value so that
    /// untrusted bytecode is rejected with a clean error instead of panicking.
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(Self::Match8),
            0x1 => Some(Self::Match16),
            0x2 => Some(Self::Match24),
            0x3 => Some(Self::Match32),
            0x4 => Some(Self::Match48),
            0x5 => Some(Self::Match64),
            0x6 => Some(Self::Call),
            0x7 => Some(Self::Return),
            0x8 => Some(Self::Trampoline),
            _ => None,
        }
    }

    /// Instruction size in bytes. The variant ladder is defined here; payload
    /// capacity and step count derive from it.
    pub const fn size(self) -> usize {
        match self {
            Self::Match8 => 8,
            Self::Match16 => 16,
            Self::Match24 => 24,
            Self::Match32 => 32,
            Self::Match48 => 48,
            Self::Match64 => 64,
            Self::Call | Self::Return | Self::Trampoline => STEP_SIZE,
        }
    }

    /// Number of steps this instruction occupies.
    pub const fn step_count(self) -> u16 {
        (self.size() / STEP_SIZE) as u16
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

    /// Payload capacity in u16 slots — whatever follows the one-step header.
    /// Zero for non-extended variants (Match8, Call, Return, Trampoline).
    pub const fn payload_slots(self) -> usize {
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
        pre_count: u8,
        neg_count: u8,
        post_count: u8,
        succ_count: u8,
        has_predicate: bool,
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
        debug_assert!(bytes.len() >= STEP_SIZE, "Match instruction too short");

        let header = bytes[0];
        let segment = header_byte::segment(header);
        let node_class = header_byte::node_class_bits(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        debug_assert!(segment == 0, "non-zero segment not yet supported");
        debug_assert!(opcode.is_match(), "expected Match opcode");

        let nav = Nav::from_byte(bytes[1]);
        let node_val = u16::from_le_bytes([bytes[2], bytes[3]]);
        let node_kind = NodeKindConstraint::from_bytes(node_class, node_val);
        let node_field = NonZeroU16::new(u16::from_le_bytes([bytes[4], bytes[5]])).map(NodeFieldId::from);

        let layout = if opcode == Opcode::Match8 {
            let next = u16::from_le_bytes([bytes[6], bytes[7]]);
            MatchLayout::Match8 { next }
        } else {
            let c = MatchCounts::unpack(u16::from_le_bytes([bytes[6], bytes[7]]));
            MatchLayout::Extended {
                pre_count: c.pre,
                neg_count: c.neg,
                post_count: c.post,
                succ_count: c.succ,
                has_predicate: c.has_predicate,
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

    /// Steps (8-byte slots) this instruction occupies, read from its opcode.
    #[inline]
    pub fn step_count(&self) -> u16 {
        header_byte::opcode(self.bytes[0])
            .expect("decoded Match has a valid opcode")
            .step_count()
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
    pub fn successor(&self, idx: usize) -> StepId {
        debug_assert!(idx < self.succ_count(), "successor index out of bounds");
        match self.layout {
            MatchLayout::Match8 { next } => {
                debug_assert!(idx == 0);
                debug_assert!(next != 0, "terminal has no successors");
                StepId::try_from(next).expect("step id must be non-zero")
            }
            MatchLayout::Extended { .. } => {
                let offset = self.succ_offset() + idx * PAYLOAD_SLOT_SIZE;
                StepId::try_from(u16::from_le_bytes([
                    self.bytes[offset],
                    self.bytes[offset + 1],
                ])).expect("step id must be non-zero")
            }
        }
    }

    /// Iterate over pre-effects (executed after transition acceptance, before post-effects).
    #[inline]
    pub fn pre_effects(&self) -> impl Iterator<Item = Effect> + '_ {
        let start = MATCH_PAYLOAD_START;
        (0..self.pre_count()).map(move |i| {
            let offset = start + i * PAYLOAD_SLOT_SIZE;
            Effect::from_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    /// Iterate over negated fields (must NOT be present on matched node).
    #[inline]
    pub fn neg_fields(&self) -> impl Iterator<Item = NodeFieldId> + '_ {
        let start = MATCH_PAYLOAD_START + self.pre_count() * PAYLOAD_SLOT_SIZE;
        (0..self.neg_count()).map(move |i| {
            let offset = start + i * PAYLOAD_SLOT_SIZE;
            let raw = u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
            NodeFieldId::try_from(raw).expect("neg field id must be non-zero")
        })
    }

    /// Iterate over post-effects (executed after successful match).
    #[inline]
    pub fn post_effects(&self) -> impl Iterator<Item = Effect> + '_ {
        let start = MATCH_PAYLOAD_START + (self.pre_count() + self.neg_count()) * PAYLOAD_SLOT_SIZE;
        (0..self.post_count()).map(move |i| {
            let offset = start + i * PAYLOAD_SLOT_SIZE;
            Effect::from_bytes([self.bytes[offset], self.bytes[offset + 1]])
        })
    }

    #[inline]
    pub fn successors(&self) -> impl Iterator<Item = StepId> + '_ {
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

    /// Get predicate data if present: (op, is_regex, value_ref).
    ///
    /// - `op`: operator (see [`crate::bytecode::predicate_op::PredicateOp`])
    /// - `is_regex`: true if value_ref is a RegexTable index, false if StringTable index
    /// - `value_ref`: index into the appropriate table
    pub fn predicate(&self) -> Option<(u8, bool, u16)> {
        if !self.has_predicate() {
            return None;
        }

        let offset = MATCH_PAYLOAD_START + self.effects_size();
        let op_and_flags = u16::from_le_bytes([self.bytes[offset], self.bytes[offset + 1]]);
        let (op, is_regex) = MatchPredicate::unpack_op_flags(op_and_flags);
        let value_ref = u16::from_le_bytes([
            self.bytes[offset + PAYLOAD_SLOT_SIZE],
            self.bytes[offset + PAYLOAD_SLOT_SIZE + 1],
        ]);

        Some((op, is_regex, value_ref))
    }

    #[inline]
    fn pre_count(&self) -> usize {
        match self.layout {
            MatchLayout::Extended { pre_count, .. } => pre_count as usize,
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

    #[inline]
    fn post_count(&self) -> usize {
        match self.layout {
            MatchLayout::Extended { post_count, .. } => post_count as usize,
            MatchLayout::Match8 { .. } => 0,
        }
    }

    /// Bytes occupied by the pre/neg/post payload slots.
    #[inline]
    fn effects_size(&self) -> usize {
        (self.pre_count() + self.neg_count() + self.post_count()) * PAYLOAD_SLOT_SIZE
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
        MATCH_PAYLOAD_START + self.effects_size() + predicate_size
    }

    /// Collect this borrowed view into an owned, encodable [`MatchInstr`].
    ///
    /// Lets the decoder be re-encoded for roundtrip testing.
    pub fn to_instr(&self) -> MatchInstr {
        MatchInstr {
            nav: self.nav,
            node_kind: self.node_kind,
            node_field: self.node_field,
            pre_effects: self.pre_effects().collect(),
            neg_fields: self.neg_fields().collect(),
            post_effects: self.post_effects().collect(),
            predicate: self.predicate().map(MatchPredicate::from_tuple),
            successors: self.successors().collect(),
        }
    }
}

/// Predicate filter carried by an extended Match (text comparison).
///
/// Mirrors the tuple returned by [`Match::predicate`].
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

    fn from_tuple((op, is_regex, value_ref): (u8, bool, u16)) -> Self {
        Self {
            op,
            is_regex,
            value_ref,
        }
    }

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
    pub pre_effects: Vec<Effect>,
    pub neg_fields: Vec<NodeFieldId>,
    pub post_effects: Vec<Effect>,
    pub predicate: Option<MatchPredicate>,
    pub successors: Vec<StepId>,
}

/// Error returned when an instruction cannot be encoded into bytecode.
///
/// Every variant is reachable from a `check`-clean query, so the emitter
/// surfaces these as compile errors instead of letting an `as`-narrowing wrap
/// or an `assert!` panic at encode time.
#[derive(Clone, PartialEq, Eq, Debug, thiserror::Error)]
pub enum EncodeError {
    #[error("too many pre-effects on one match: {0} (max {MAX_PRE_EFFECTS})")]
    TooManyPreEffects(usize),
    #[error("too many negated fields on one match: {0} (max {MAX_NEG_FIELDS})")]
    TooManyNegFields(usize),
    #[error("too many post-effects on one match: {0} (max {MAX_POST_EFFECTS})")]
    TooManyPostEffects(usize),
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
        for effect in self.pre_effects.iter().chain(&self.post_effects) {
            if effect.payload > EFFECT_PAYLOAD_MAX {
                return Err(EncodeError::EffectPayloadOverflow(effect.payload));
            }
        }

        let (node_class, node_val) = self.node_kind.to_bytes();
        let node_field_val = self.node_field.map_or(0, u16::from);

        let can_use_match8 = self.pre_effects.is_empty()
            && self.neg_fields.is_empty()
            && self.post_effects.is_empty()
            && self.predicate.is_none()
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

        let pre = self.pre_effects.len();
        let neg = self.neg_fields.len();
        let post = self.post_effects.len();
        let succ = self.successors.len();
        if pre > MAX_PRE_EFFECTS {
            return Err(EncodeError::TooManyPreEffects(pre));
        }
        if neg > MAX_NEG_FIELDS {
            return Err(EncodeError::TooManyNegFields(neg));
        }
        if post > MAX_POST_EFFECTS {
            return Err(EncodeError::TooManyPostEffects(post));
        }
        if succ > MAX_SUCCESSORS {
            return Err(EncodeError::TooManySuccessors(succ));
        }

        let predicate_slots = if self.predicate.is_some() {
            PREDICATE_SLOTS
        } else {
            0
        };
        let slots = pre + neg + post + predicate_slots + succ;
        let opcode = select_match_opcode(slots).ok_or(EncodeError::PayloadTooLarge(slots))?;

        let mut bytes = vec![0u8; opcode.size()];
        bytes[0] = header_byte::pack(0, node_class, opcode);
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&node_val.to_le_bytes());
        bytes[4..6].copy_from_slice(&node_field_val.to_le_bytes());

        let counts = MatchCounts {
            pre: pre as u8,
            neg: neg as u8,
            post: post as u8,
            succ: succ as u8,
            has_predicate: self.predicate.is_some(),
        };
        bytes[6..8].copy_from_slice(&counts.pack().to_le_bytes());

        let mut offset = MATCH_PAYLOAD_START;
        let mut put = |bytes: &mut [u8], data: [u8; 2]| {
            bytes[offset..offset + PAYLOAD_SLOT_SIZE].copy_from_slice(&data);
            offset += PAYLOAD_SLOT_SIZE;
        };
        for effect in &self.pre_effects {
            put(&mut bytes, effect.to_bytes());
        }
        for &field in &self.neg_fields {
            put(&mut bytes, u16::from(field).to_le_bytes());
        }
        for effect in &self.post_effects {
            put(&mut bytes, effect.to_bytes());
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

/// Call instruction for invoking definitions (recursion).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Call {
    /// Segment index (0-3).
    pub segment: u8,
    /// Navigation to apply before jumping to target.
    pub nav: Nav,
    /// Field constraint (None = no constraint).
    pub node_field: Option<NodeFieldId>,
    /// Return address (current segment).
    pub next: StepId,
    /// Callee entry point (target segment from type_id).
    pub target: StepId,
}

impl Call {
    pub fn new(nav: Nav, node_field: Option<NodeFieldId>, next: StepId, target: StepId) -> Self {
        Self {
            segment: 0,
            nav,
            node_field,
            next,
            target,
        }
    }

    /// Decode from 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    /// For Call, node_class bits are ignored (always 0).
    pub(crate) fn from_bytes(bytes: [u8; 8]) -> Self {
        let header = bytes[0];
        let segment = header_byte::segment(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        assert_eq!(opcode, Opcode::Call, "expected Call opcode");

        Self {
            segment,
            nav: Nav::from_byte(bytes[1]),
            node_field: NonZeroU16::new(u16::from_le_bytes([bytes[2], bytes[3]])).map(NodeFieldId::from),
            next: StepId::try_from(u16::from_le_bytes([bytes[4], bytes[5]])).expect("step id must be non-zero"),
            target: StepId::try_from(u16::from_le_bytes([bytes[6], bytes[7]])).expect("step id must be non-zero"),
        }
    }

    /// Encode to 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = header_byte::pack(self.segment, 0, Opcode::Call);
        bytes[1] = self.nav.to_byte();
        bytes[2..4].copy_from_slice(&self.node_field.map_or(0, u16::from).to_le_bytes());
        bytes[4..6].copy_from_slice(&u16::from(self.next).to_le_bytes());
        bytes[6..8].copy_from_slice(&u16::from(self.target).to_le_bytes());
        bytes
    }

    pub fn nav(&self) -> Nav {
        self.nav
    }
    pub fn node_field(&self) -> Option<NodeFieldId> {
        self.node_field
    }
    pub fn next(&self) -> StepId {
        self.next
    }
    pub fn target(&self) -> StepId {
        self.target
    }
}

/// Return instruction for returning from definitions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Return {
    /// Segment index (0-3).
    pub segment: u8,
}

impl Return {
    pub fn new() -> Self {
        Self { segment: 0 }
    }

    /// Decode from 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    /// For Return, node_class bits are ignored (always 0).
    pub(crate) fn from_bytes(bytes: [u8; 8]) -> Self {
        let header = bytes[0];
        let segment = header_byte::segment(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        assert_eq!(opcode, Opcode::Return, "expected Return opcode");

        Self { segment }
    }

    /// Encode to 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = header_byte::pack(self.segment, 0, Opcode::Return);
        // bytes[1..8] are reserved/padding
        bytes
    }
}

impl Default for Return {
    fn default() -> Self {
        Self::new()
    }
}

/// Trampoline instruction for universal entry.
///
/// Like Call, but the target comes from VM context (external parameter)
/// rather than being encoded in the instruction. Used at address 0 for
/// the entry preamble: `StructOpen → Trampoline → StructClose → Accept`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trampoline {
    /// Segment index (0-3).
    pub segment: u8,
    /// Return address (where to continue after entrypoint returns).
    pub next: StepId,
}

impl Trampoline {
    pub fn new(next: StepId) -> Self {
        Self { segment: 0, next }
    }

    /// Decode from 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    /// For Trampoline, node_class bits are ignored (always 0).
    pub(crate) fn from_bytes(bytes: [u8; 8]) -> Self {
        let header = bytes[0];
        let segment = header_byte::segment(header);
        let opcode = header_byte::opcode(header).expect("invalid opcode");
        assert!(
            segment == 0,
            "non-zero segment not yet supported: {segment}"
        );
        assert_eq!(opcode, Opcode::Trampoline, "expected Trampoline opcode");

        Self {
            segment,
            next: StepId::try_from(u16::from_le_bytes([bytes[2], bytes[3]])).expect("step id must be non-zero"),
        }
    }

    /// Encode to 8-byte bytecode.
    ///
    /// Header byte layout: `segment(2) | node_class(2) | opcode(4)`
    pub fn to_bytes(&self) -> [u8; 8] {
        let mut bytes = [0u8; 8];
        bytes[0] = header_byte::pack(self.segment, 0, Opcode::Trampoline);
        // bytes[1] is padding
        bytes[2..4].copy_from_slice(&u16::from(self.next).to_le_bytes());
        // bytes[4..8] are reserved/padding
        bytes
    }

    pub fn next(&self) -> StepId {
        self.next
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

/// Pad a size to the next multiple of SECTION_ALIGN (64 bytes).
#[inline]
#[cfg(test)]
pub fn align_to_section(size: usize) -> usize {
    (size + SECTION_ALIGN - 1) & !(SECTION_ALIGN - 1)
}
