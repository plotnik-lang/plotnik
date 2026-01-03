//! Navigation command encoding for bytecode instructions.
//!
//! Navigation determines how the VM moves through the tree-sitter AST.

/// Navigation command for VM execution.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Nav {
    #[default]
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
                0 => Self::Stay,
                1 => Self::StayExact,
                2 => Self::Next,
                3 => Self::NextSkip,
                4 => Self::NextExact,
                5 => Self::Down,
                6 => Self::DownSkip,
                7 => Self::DownExact,
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
            Self::Stay => 0,
            Self::StayExact => 1,
            Self::Next => 2,
            Self::NextSkip => 3,
            Self::NextExact => 4,
            Self::Down => 5,
            Self::DownSkip => 6,
            Self::DownExact => 7,
            Self::Up(n) => {
                debug_assert!((1..=63).contains(&n));
                0b01_000000 | n
            }
            Self::UpSkipTrivia(n) => {
                debug_assert!((1..=63).contains(&n));
                0b10_000000 | n
            }
            Self::UpExact(n) => {
                debug_assert!((1..=63).contains(&n));
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
            Self::Down | Self::DownSkip => Self::DownExact,
            Self::Next | Self::NextSkip => Self::NextExact,
            Self::Stay => Self::StayExact,
            Self::Up(n) | Self::UpSkipTrivia(n) => Self::UpExact(n),
            // Already exact variants
            Self::DownExact | Self::NextExact | Self::StayExact | Self::UpExact(_) => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_standard_roundtrip() {
        for nav in [
            Nav::Stay,
            Nav::StayExact,
            Nav::Next,
            Nav::NextSkip,
            Nav::NextExact,
            Nav::Down,
            Nav::DownSkip,
            Nav::DownExact,
        ] {
            assert_eq!(Nav::from_byte(nav.to_byte()), nav);
        }
    }

    #[test]
    fn nav_up_roundtrip() {
        let nav = Nav::Up(5);
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);

        let nav = Nav::UpSkipTrivia(10);
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);

        let nav = Nav::UpExact(63);
        assert_eq!(Nav::from_byte(nav.to_byte()), nav);
    }

    #[test]
    fn nav_byte_encoding() {
        assert_eq!(Nav::Stay.to_byte(), 0b00_000000);
        assert_eq!(Nav::StayExact.to_byte(), 0b00_000001);
        assert_eq!(Nav::Down.to_byte(), 0b00_000101);
        assert_eq!(Nav::Up(5).to_byte(), 0b01_000101);
        assert_eq!(Nav::UpSkipTrivia(3).to_byte(), 0b10_000011);
        assert_eq!(Nav::UpExact(1).to_byte(), 0b11_000001);
    }

    #[test]
    #[should_panic(expected = "invalid nav standard")]
    fn nav_invalid_standard_panics() {
        Nav::from_byte(0b00_111111);
    }

    #[test]
    #[should_panic(expected = "invalid nav up level")]
    fn nav_invalid_up_zero_panics() {
        Nav::from_byte(0b01_000000);
    }
}
