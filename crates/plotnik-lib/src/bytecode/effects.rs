//! Effect operations for bytecode.

/// An effect word packs the kind in the high bits and a payload in the low
/// [`EFFECT_PAYLOAD_BITS`].
pub const EFFECT_PAYLOAD_BITS: u32 = 10;

/// Largest representable effect payload (the low [`EFFECT_PAYLOAD_BITS`]).
pub const EFFECT_PAYLOAD_MAX: usize = (1 << EFFECT_PAYLOAD_BITS) - 1;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
pub enum EffectKind {
    Node = 0,
    ListOpen = 1,
    ArrayPush = 2,
    ListClose = 3,
    RecordOpen = 4,
    RecordSet = 5,
    RecordClose = 6,
    VariantOpen = 7,
    VariantClose = 8,
    Absent = 9,
    SuppressBegin = 10,
    SuppressEnd = 11,
    /// Open an inspection span and snapshot the current cursor node.
    SpanStartAt = 12,
    /// Open an inspection span without reading the cursor.
    SpanStart = 13,
    /// Close the innermost inspection span.
    SpanEnd = 14,
    /// Open one value-local scalar provenance frame.
    ScalarOpen = 15,
    /// Mark the current explicit node-pattern match in every open scalar frame.
    ScalarMark = 16,
    /// Close a scalar frame and produce source text (or null with no marks).
    TextClose = 17,
    /// Close a scalar frame and produce the boolean encoded in the payload.
    BoolClose = 18,
    /// Produce the current node's source text directly.
    NodeText = 19,
    /// Produce `true` for the current matched node directly.
    NodeBool = 20,
    /// Produce the boolean encoded in the payload without provenance.
    BoolValue = 21,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectSuppression {
    Output,
    Control,
    Bypass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueFrameKind {
    List,
    Record,
    Variant,
    Scalar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameAction {
    Open(ValueFrameKind),
    Close(ValueFrameKind),
}

impl EffectKind {
    fn from_u8(v: u8) -> Self {
        Self::try_from_u8(v).unwrap_or_else(|| panic!("invalid effect opcode: {v}"))
    }

    /// Non-panicking decode, for validating an untrusted instruction stream at
    /// load time before the VM decodes its effect payload.
    pub(crate) fn try_from_u8(v: u8) -> Option<Self> {
        let op = match v {
            0 => Self::Node,
            1 => Self::ListOpen,
            2 => Self::ArrayPush,
            3 => Self::ListClose,
            4 => Self::RecordOpen,
            5 => Self::RecordSet,
            6 => Self::RecordClose,
            7 => Self::VariantOpen,
            8 => Self::VariantClose,
            9 => Self::Absent,
            10 => Self::SuppressBegin,
            11 => Self::SuppressEnd,
            12 => Self::SpanStartAt,
            13 => Self::SpanStart,
            14 => Self::SpanEnd,
            15 => Self::ScalarOpen,
            16 => Self::ScalarMark,
            17 => Self::TextClose,
            18 => Self::BoolClose,
            19 => Self::NodeText,
            20 => Self::NodeBool,
            21 => Self::BoolValue,
            _ => return None,
        };
        Some(op)
    }

    pub fn suppression(self) -> EffectSuppression {
        match self {
            Self::SuppressBegin | Self::SuppressEnd => EffectSuppression::Control,
            Self::SpanStartAt | Self::SpanStart | Self::SpanEnd | Self::ScalarMark => {
                EffectSuppression::Bypass
            }
            _ => EffectSuppression::Output,
        }
    }

    pub fn reads_cursor(self) -> bool {
        matches!(
            self,
            Self::Node | Self::SpanStartAt | Self::ScalarMark | Self::NodeText | Self::NodeBool
        )
    }

    pub fn is_motion_barrier(self) -> bool {
        matches!(self, Self::ScalarOpen | Self::TextClose | Self::BoolClose)
    }

    pub fn frame_action(self) -> Option<FrameAction> {
        let action = match self {
            Self::ListOpen => FrameAction::Open(ValueFrameKind::List),
            Self::ListClose => FrameAction::Close(ValueFrameKind::List),
            Self::RecordOpen => FrameAction::Open(ValueFrameKind::Record),
            Self::RecordClose => FrameAction::Close(ValueFrameKind::Record),
            Self::VariantOpen => FrameAction::Open(ValueFrameKind::Variant),
            Self::VariantClose => FrameAction::Close(ValueFrameKind::Variant),
            Self::ScalarOpen => FrameAction::Open(ValueFrameKind::Scalar),
            Self::TextClose | Self::BoolClose => FrameAction::Close(ValueFrameKind::Scalar),
            _ => return None,
        };
        Some(action)
    }

    /// Validate the payload using the two tables an effect may index. All
    /// effects decoded at the trust boundary pass through this one contract.
    pub fn accepts_payload(self, payload: usize, member_count: usize, span_count: usize) -> bool {
        match self {
            Self::RecordSet | Self::VariantOpen => payload < member_count,
            Self::SpanStartAt | Self::SpanStart | Self::SpanEnd => payload < span_count,
            Self::BoolClose | Self::BoolValue => payload <= 1,
            _ => payload == 0,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Effect {
    pub kind: EffectKind,
    pub payload: usize,
}

impl Effect {
    pub fn new(kind: EffectKind, payload: usize) -> Self {
        Self { kind, payload }
    }

    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        let raw = u16::from_le_bytes(bytes);
        let kind = EffectKind::from_u8((raw >> EFFECT_PAYLOAD_BITS) as u8);
        let payload = (raw & EFFECT_PAYLOAD_MAX as u16) as usize;
        Self { kind, payload }
    }

    /// Non-panicking decode, for validating an untrusted instruction stream at
    /// load time. Returns `None` when the kind field is not a known effect.
    pub(crate) fn try_from_bytes(bytes: [u8; 2]) -> Option<Self> {
        let raw = u16::from_le_bytes(bytes);
        let kind = EffectKind::try_from_u8((raw >> EFFECT_PAYLOAD_BITS) as u8)?;
        let payload = (raw & EFFECT_PAYLOAD_MAX as u16) as usize;
        Some(Self { kind, payload })
    }

    pub fn to_bytes(self) -> [u8; 2] {
        assert!(
            self.payload <= EFFECT_PAYLOAD_MAX,
            "effect payload exceeds {EFFECT_PAYLOAD_BITS}-bit limit: {}",
            self.payload
        );
        let raw = ((self.kind as u16) << EFFECT_PAYLOAD_BITS)
            | (self.payload as u16 & EFFECT_PAYLOAD_MAX as u16);
        raw.to_le_bytes()
    }
}
