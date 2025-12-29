//! Effect operations for bytecode.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum EffectOpcode {
    Node = 0,
    A = 1,
    Push = 2,
    EndA = 3,
    S = 4,
    EndS = 5,
    Set = 6,
    E = 7,
    EndE = 8,
    Text = 9,
    Clear = 10,
    Null = 11,
}

impl EffectOpcode {
    fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Node,
            1 => Self::A,
            2 => Self::Push,
            3 => Self::EndA,
            4 => Self::S,
            5 => Self::EndS,
            6 => Self::Set,
            7 => Self::E,
            8 => Self::EndE,
            9 => Self::Text,
            10 => Self::Clear,
            11 => Self::Null,
            _ => panic!("invalid effect opcode: {v}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EffectOp {
    pub opcode: EffectOpcode,
    pub payload: usize,
}

impl EffectOp {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_payload() {
        let op = EffectOp {
            opcode: EffectOpcode::Set,
            payload: 42,
        };
        let bytes = op.to_bytes();
        let decoded = EffectOp::from_bytes(bytes);
        assert_eq!(decoded.opcode, EffectOpcode::Set);
        assert_eq!(decoded.payload, 42);
    }

    #[test]
    fn roundtrip_no_payload() {
        let op = EffectOp {
            opcode: EffectOpcode::Node,
            payload: 0,
        };
        let bytes = op.to_bytes();
        let decoded = EffectOp::from_bytes(bytes);
        assert_eq!(decoded.opcode, EffectOpcode::Node);
        assert_eq!(decoded.payload, 0);
    }

    #[test]
    fn max_payload() {
        let op = EffectOp {
            opcode: EffectOpcode::E,
            payload: 1023,
        };
        let bytes = op.to_bytes();
        let decoded = EffectOp::from_bytes(bytes);
        assert_eq!(decoded.payload, 1023);
    }

    #[test]
    #[should_panic(expected = "invalid effect opcode")]
    fn invalid_opcode_panics() {
        let bytes = [0xFF, 0xFF]; // opcode would be 63, which is invalid
        EffectOp::from_bytes(bytes);
    }
}
