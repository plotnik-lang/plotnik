//! Navigation command encoding for bytecode instructions.
//!
//! Navigation determines how the VM moves through the tree-sitter AST.

/// Navigation command for VM execution.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub enum Nav {
    /// Epsilon transition: pure control flow, no cursor movement or node check.
    /// Used for branching, quantifier loops, and effect-only transitions.
    #[default]
    Epsilon,
    /// Stay at current position.
    Stay,
    /// Stay at current position, exact match only (no continue_search).
    StayExact,
    Next,
    NextSkip,
    NextExact,
    Down,
    DownSkip,
    DownExact,
    Up(u8),
    UpSkipTrivia(u8),
    UpExact(u8),
}

impl Nav {
    /// Decode from bytecode byte.
    ///
    /// Byte layout:
    /// - Bits 7-6: Mode (00=Standard, 01=Up, 10=UpSkipTrivia, 11=UpExact)
    /// - Bits 5-0: Payload (enum value for Standard, level count for Up variants)
    pub fn from_byte(b: u8) -> Self {
        let mode = b >> 6;
        let payload = b & 0x3F;

        match mode {
            0b00 => match payload {
                0 => Self::Epsilon,
                1 => Self::Stay,
                2 => Self::StayExact,
                3 => Self::Next,
                4 => Self::NextSkip,
                5 => Self::NextExact,
                6 => Self::Down,
                7 => Self::DownSkip,
                8 => Self::DownExact,
                _ => panic!("invalid nav standard: {payload}"),
            },
            0b01 => {
                assert!(payload >= 1, "invalid nav up level: {payload}");
                Self::Up(payload)
            }
            0b10 => {
                assert!(payload >= 1, "invalid nav up_skip_trivia level: {payload}");
                Self::UpSkipTrivia(payload)
            }
            0b11 => {
                assert!(payload >= 1, "invalid nav up_exact level: {payload}");
                Self::UpExact(payload)
            }
            _ => unreachable!(),
        }
    }

    /// Encode to bytecode byte.
    pub fn to_byte(self) -> u8 {
        match self {
            Self::Epsilon => 0,
            Self::Stay => 1,
            Self::StayExact => 2,
            Self::Next => 3,
            Self::NextSkip => 4,
            Self::NextExact => 5,
            Self::Down => 6,
            Self::DownSkip => 7,
            Self::DownExact => 8,
            Self::Up(n) => {
                assert!((1..=63).contains(&n), "Up level overflow: {n} > 63");
                0b01_000000 | n
            }
            Self::UpSkipTrivia(n) => {
                assert!(
                    (1..=63).contains(&n),
                    "UpSkipTrivia level overflow: {n} > 63"
                );
                0b10_000000 | n
            }
            Self::UpExact(n) => {
                assert!((1..=63).contains(&n), "UpExact level overflow: {n} > 63");
                0b11_000000 | n
            }
        }
    }

    /// Convert navigation to its exact variant (no search loop).
    ///
    /// Used by alternation branches which should match at their exact
    /// cursor position only - the search among positions is owned by
    /// the parent context (quantifier's skip-retry, sequence advancement).
    pub fn to_exact(self) -> Self {
        match self {
            Self::Epsilon => Self::Epsilon, // Epsilon stays epsilon
            Self::Down | Self::DownSkip => Self::DownExact,
            Self::Next | Self::NextSkip => Self::NextExact,
            Self::Stay => Self::StayExact,
            Self::Up(n) | Self::UpSkipTrivia(n) => Self::UpExact(n),
            // Already exact variants
            Self::DownExact | Self::NextExact | Self::StayExact | Self::UpExact(_) => self,
        }
    }
}
