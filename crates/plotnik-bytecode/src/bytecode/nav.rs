//! Navigation command encoding for bytecode instructions.
//!
//! Navigation determines how the VM moves through the tree-sitter AST.

/// Navigation command for VM execution.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
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
    NextSkipExtras,
    NextExact,
    Down,
    DownSkip,
    DownSkipExtras,
    DownExact,
    Up(u8),
    UpSkipTrivia(u8),
    UpSkipExtras(u8),
    UpExact(u8),
}

const UP_SKIP_EXTRAS_BASE: u8 = 10;
const MAX_UP_SKIP_EXTRAS_LEVEL: u8 = 63 - UP_SKIP_EXTRAS_BASE;

impl Nav {
    /// Decode from bytecode byte.
    ///
    /// Byte layout:
    /// - Bits 7-6: Mode (00=Standard/UpSkipExtras, 01=Up, 10=UpSkipTrivia, 11=UpExact)
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
                5 => Self::NextSkipExtras,
                6 => Self::NextExact,
                7 => Self::Down,
                8 => Self::DownSkip,
                9 => Self::DownSkipExtras,
                10 => Self::DownExact,
                _ => Self::UpSkipExtras(payload - UP_SKIP_EXTRAS_BASE),
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
            Self::NextSkipExtras => 5,
            Self::NextExact => 6,
            Self::Down => 7,
            Self::DownSkip => 8,
            Self::DownSkipExtras => 9,
            Self::DownExact => 10,
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
            Self::UpSkipExtras(n) => {
                assert!(
                    (1..=MAX_UP_SKIP_EXTRAS_LEVEL).contains(&n),
                    "UpSkipExtras level overflow: {n} > {MAX_UP_SKIP_EXTRAS_LEVEL}"
                );
                UP_SKIP_EXTRAS_BASE + n
            }
            Self::UpExact(n) => {
                assert!((1..=63).contains(&n), "UpExact level overflow: {n} > 63");
                0b11_000000 | n
            }
        }
    }

    /// Navigation that advances to the next sibling while preserving this nav's
    /// skip policy.
    ///
    /// Shared source of truth for the two places that step a sibling search
    /// forward: the VM's in-instruction `continue_search`, and the compiler's
    /// quantifier repeat iteration. A `Down*` entry and its `Next*` continuation
    /// share a skip policy (trivia/extras/exact), so both map to the same
    /// `Next*`; navs that drive no sibling search default to `Next`.
    pub fn sibling_continuation(self) -> Self {
        match self {
            Self::Down | Self::Next => Self::Next,
            Self::DownSkip | Self::NextSkip => Self::NextSkip,
            Self::DownSkipExtras | Self::NextSkipExtras => Self::NextSkipExtras,
            Self::DownExact | Self::NextExact => Self::NextExact,
            Self::Epsilon
            | Self::Stay
            | Self::StayExact
            | Self::Up(_)
            | Self::UpSkipTrivia(_)
            | Self::UpSkipExtras(_)
            | Self::UpExact(_) => Self::Next,
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
            Self::Down | Self::DownSkip | Self::DownSkipExtras => Self::DownExact,
            Self::Next | Self::NextSkip | Self::NextSkipExtras => Self::NextExact,
            Self::Stay => Self::StayExact,
            Self::Up(n) | Self::UpSkipTrivia(n) | Self::UpSkipExtras(n) => Self::UpExact(n),
            // Already exact variants
            Self::DownExact | Self::NextExact | Self::StayExact | Self::UpExact(_) => self,
        }
    }
}
