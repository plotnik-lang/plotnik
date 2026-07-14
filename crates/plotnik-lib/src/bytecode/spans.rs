//! Inspection span metadata section.

use crate::bytecode::SPAN_ENTRY_SIZE;

/// Classifies the query construct covered by an inspection span.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum SpanKind {
    Def,
    Ref,
    Pattern,
    Capture,
    GrammarField,
    NegatedGrammarField,
    Predicate,
    Quantifier,
    Sequence,
    Alternation(Labeling),
    Alternative,
    CaptureType,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Labeling {
    Unlabeled,
    Labeled,
}

impl SpanKind {
    pub(crate) fn try_from_u8(v: u8) -> Option<Self> {
        let kind = match v {
            0 => Self::Def,
            1 => Self::Ref,
            2 => Self::Pattern,
            3 => Self::Capture,
            4 => Self::GrammarField,
            5 => Self::NegatedGrammarField,
            6 => Self::Predicate,
            7 => Self::Quantifier,
            8 => Self::Sequence,
            9 => Self::Alternation(Labeling::Unlabeled),
            10 => Self::Alternation(Labeling::Labeled),
            11 => Self::Alternative,
            12 => Self::CaptureType,
            _ => return None,
        };
        Some(kind)
    }

    fn to_u8(self) -> u8 {
        match self {
            Self::Def => 0,
            Self::Ref => 1,
            Self::Pattern => 2,
            Self::Capture => 3,
            Self::GrammarField => 4,
            Self::NegatedGrammarField => 5,
            Self::Predicate => 6,
            Self::Quantifier => 7,
            Self::Sequence => 8,
            Self::Alternation(Labeling::Unlabeled) => 9,
            Self::Alternation(Labeling::Labeled) => 10,
            Self::Alternative => 11,
            Self::CaptureType => 12,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Def => "def",
            Self::Ref => "ref",
            Self::Pattern => "pattern",
            Self::Capture => "capture",
            Self::GrammarField => "grammar_field",
            Self::NegatedGrammarField => "negated_grammar_field",
            Self::Predicate => "predicate",
            Self::Quantifier => "quantifier",
            Self::Sequence => "sequence",
            Self::Alternation(Labeling::Unlabeled) => "unlabeled_alternation",
            Self::Alternation(Labeling::Labeled) => "labeled_alternation",
            Self::Alternative => "alternative",
            Self::CaptureType => "capture_type",
        }
    }
}

/// No type/member binding for a span entry.
pub const SPAN_NO_BINDING: u16 = 0xFFFF;

/// One decoded entry of the `spans` section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpanEntry {
    pub source_id: u16,
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
            source_id: u16::from_le_bytes([bytes[0], bytes[1]]),
            kind: SpanKind::try_from_u8(bytes[2]).expect("span kind validated at load"),
            start: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            end: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            type_id: u16::from_le_bytes([bytes[12], bytes[13]]),
            member: u16::from_le_bytes([bytes[14], bytes[15]]),
        }
    }

    pub(crate) fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..2].copy_from_slice(&self.source_id.to_le_bytes());
        bytes[2] = self.kind.to_u8();
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
