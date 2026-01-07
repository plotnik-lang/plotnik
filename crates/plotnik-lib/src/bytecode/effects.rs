//! Effect operations for bytecode.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum EffectOpcode {
    Node = 0,
    Arr = 1,
    Push = 2,
    EndArr = 3,
    Obj = 4,
    EndObj = 5,
    Set = 6,
    Enum = 7,
    EndEnum = 8,
    Text = 9,
    Clear = 10,
    Null = 11,
    SuppressBegin = 12,
    SuppressEnd = 13,
}

impl EffectOpcode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Node,
            1 => Self::Arr,
            2 => Self::Push,
            3 => Self::EndArr,
            4 => Self::Obj,
            5 => Self::EndObj,
            6 => Self::Set,
            7 => Self::Enum,
            8 => Self::EndEnum,
            9 => Self::Text,
            10 => Self::Clear,
            11 => Self::Null,
            12 => Self::SuppressBegin,
            13 => Self::SuppressEnd,
            _ => panic!("invalid effect opcode: {v}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectOp {
    pub(crate) opcode: EffectOpcode,
    pub(crate) payload: usize,
}

impl EffectOp {
    /// Create a new effect operation.
    pub fn new(opcode: EffectOpcode, payload: usize) -> Self {
        Self { opcode, payload }
    }

    pub fn from_bytes(bytes: [u8; 2]) -> Self {
        let raw = u16::from_le_bytes(bytes);
        let opcode = EffectOpcode::from_u8((raw >> 10) as u8);
        let payload = (raw & 0x3FF) as usize;
        Self { opcode, payload }
    }

    pub fn to_bytes(self) -> [u8; 2] {
        assert!(
            self.payload <= 0x3FF,
            "effect payload exceeds 10-bit limit: {}",
            self.payload
        );
        let raw = ((self.opcode as u16) << 10) | ((self.payload as u16) & 0x3FF);
        raw.to_le_bytes()
    }

    pub fn opcode(&self) -> EffectOpcode {
        self.opcode
    }
    pub fn payload(&self) -> usize {
        self.payload
    }
}
