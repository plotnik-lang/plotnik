//! Inspection span metadata section.

use crate::bytecode::SPAN_ENTRY_SIZE;

/// Classifies the query construct covered by an inspection span.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[repr(u8)]
pub enum SpanKind {
    Def = 0,
    Ref = 1,
    Pattern = 2,
    Capture = 3,
    Field = 4,
    NegField = 5,
    Predicate = 6,
    Quantifier = 7,
    Sequence = 8,
    Union = 9,
    Enum = 10,
    Branch = 11,
    Annotation = 12,
}

impl SpanKind {
    pub(crate) fn try_from_u8(v: u8) -> Option<Self> {
        let kind = match v {
            0 => Self::Def,
            1 => Self::Ref,
            2 => Self::Pattern,
            3 => Self::Capture,
            4 => Self::Field,
            5 => Self::NegField,
            6 => Self::Predicate,
            7 => Self::Quantifier,
            8 => Self::Sequence,
            9 => Self::Union,
            10 => Self::Enum,
            11 => Self::Branch,
            12 => Self::Annotation,
            _ => return None,
        };
        Some(kind)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Def => "def",
            Self::Ref => "ref",
            Self::Pattern => "pattern",
            Self::Capture => "capture",
            Self::Field => "field",
            Self::NegField => "neg_field",
            Self::Predicate => "predicate",
            Self::Quantifier => "quantifier",
            Self::Sequence => "sequence",
            Self::Union => "union",
            Self::Enum => "enum",
            Self::Branch => "branch",
            Self::Annotation => "annotation",
        }
    }
}

/// No type/member binding for a span entry.
pub const SPAN_NO_BINDING: u16 = 0xFFFF;

/// One decoded entry of the `spans` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanEntry {
    pub source: u16,
    pub kind: SpanKind,
    pub start: u32,
    pub end: u32,
    /// Bound bytecode type, or [`SPAN_NO_BINDING`].
    pub type_id: u16,
    /// Absolute index into the TypeMembers section, or [`SPAN_NO_BINDING`].
    pub member: u16,
}

impl SpanEntry {
    pub const SIZE: usize = SPAN_ENTRY_SIZE;

    pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
        assert!(
            bytes.len() >= Self::SIZE,
            "span entry requires {} bytes",
            Self::SIZE
        );
        Self {
            source: u16::from_le_bytes([bytes[0], bytes[1]]),
            kind: SpanKind::try_from_u8(bytes[2]).expect("span kind validated at load"),
            start: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            end: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            type_id: u16::from_le_bytes([bytes[12], bytes[13]]),
            member: u16::from_le_bytes([bytes[14], bytes[15]]),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..2].copy_from_slice(&self.source.to_le_bytes());
        bytes[2] = self.kind as u8;
        bytes[3] = 0;
        bytes[4..8].copy_from_slice(&self.start.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.end.to_le_bytes());
        bytes[12..14].copy_from_slice(&self.type_id.to_le_bytes());
        bytes[14..16].copy_from_slice(&self.member.to_le_bytes());
        bytes
    }
}

/// View into the spans section (index = SpanId).
pub struct SpansView<'a> {
    bytes: &'a [u8],
    count: usize,
}

impl<'a> SpansView<'a> {
    pub(crate) fn new(bytes: &'a [u8], count: usize) -> Self {
        Self { bytes, count }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn get(&self, idx: usize) -> SpanEntry {
        assert!(idx < self.count, "span index out of bounds");
        let offset = idx * SpanEntry::SIZE;
        SpanEntry::from_bytes(&self.bytes[offset..offset + SpanEntry::SIZE])
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = SpanEntry> + '_ {
        (0..self.count).map(|i| self.get(i))
    }
}
