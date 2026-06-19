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
    ArrayOpen = 1,
    Push = 2,
    ArrayClose = 3,
    StructOpen = 4,
    StructClose = 5,
    Set = 6,
    EnumOpen = 7,
    EnumClose = 8,
    Null = 9,
    SuppressBegin = 10,
    SuppressEnd = 11,
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
            1 => Self::ArrayOpen,
            2 => Self::Push,
            3 => Self::ArrayClose,
            4 => Self::StructOpen,
            5 => Self::StructClose,
            6 => Self::Set,
            7 => Self::EnumOpen,
            8 => Self::EnumClose,
            9 => Self::Null,
            10 => Self::SuppressBegin,
            11 => Self::SuppressEnd,
            _ => return None,
        };
        Some(op)
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
