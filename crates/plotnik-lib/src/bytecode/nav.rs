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

/// Bit 7 of a Nav byte marks the Up family; a clear bit 7 is a standard command.
const UP_FLAG: u8 = 0b1000_0000;
/// The Up mode tag occupies bits 6-5, just above the level field.
const UP_MODE_SHIFT: u8 = 5;

/// Up mode tags (bits 6-5 of an Up byte), in `Nav` declaration order.
const UP_ANY: u8 = 0b00;
const UP_SKIP_TRIVIA: u8 = 0b01;
const UP_SKIP_EXTRAS: u8 = 0b10;
const UP_EXACT: u8 = 0b11;

impl Nav {
    /// Largest level a single `Up*` instruction can encode. The level lives in
    /// bits 4-0, so the range is `1..=31`. The compiler splits a deeper ascent
    /// into a chain of same-mode `Up*` instructions, which is exact because
    /// `Up*` composes — the VM re-checks the exit constraint at every level it
    /// leaves (see `the VM`'s `go_up`).
    pub const MAX_UP_LEVEL: u8 = (1 << UP_MODE_SHIFT) - 1;

    /// Decode from a bytecode byte, panicking on an invalid encoding.
    ///
    /// Byte layout:
    /// - Bit 7 set — an Up command: bits 6-5 are the mode (`00` Up, `01`
    ///   UpSkipTrivia, `10` UpSkipExtras, `11` UpExact), bits 4-0 the level
    ///   (`1..=31`).
    /// - Bit 7 clear — a standard command: bits 6-0 are its enum value (`0..=10`).
    pub fn from_byte(b: u8) -> Self {
        Self::try_from_byte(b).unwrap_or_else(|| panic!("invalid nav byte: {b:#04x}"))
    }

    /// Non-panicking nav decode, for validating an untrusted instruction stream
    /// at load time before the VM decodes it. The invalid encodings are an Up
    /// byte with a zero level and a standard byte whose enum value is unassigned
    /// (`11..=127`).
    pub fn try_from_byte(b: u8) -> Option<Self> {
        if b & UP_FLAG != 0 {
            let level = b & Self::MAX_UP_LEVEL;
            if level == 0 {
                return None;
            }
            let nav = match (b >> UP_MODE_SHIFT) & 0b11 {
                UP_ANY => Self::Up(level),
                UP_SKIP_TRIVIA => Self::UpSkipTrivia(level),
                UP_SKIP_EXTRAS => Self::UpSkipExtras(level),
                _ => Self::UpExact(level), // UP_EXACT
            };
            return Some(nav);
        }

        let nav = match b {
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
            _ => return None,
        };
        Some(nav)
    }

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
            Self::Up(n) => Self::up_byte(UP_ANY, n),
            Self::UpSkipTrivia(n) => Self::up_byte(UP_SKIP_TRIVIA, n),
            Self::UpSkipExtras(n) => Self::up_byte(UP_SKIP_EXTRAS, n),
            Self::UpExact(n) => Self::up_byte(UP_EXACT, n),
        }
    }

    /// Cursor-depth delta this navigation applies to the abstract AST cursor.
    ///
    /// The verifier only tracks depth, not sibling position: `Down*` enters a
    /// child, `Up*` exits one or more parents, and stay/next/epsilon variants
    /// keep the current depth.
    pub(crate) const fn depth_delta(self) -> i32 {
        match self {
            Self::Epsilon
            | Self::Stay
            | Self::StayExact
            | Self::Next
            | Self::NextSkip
            | Self::NextSkipExtras
            | Self::NextExact => 0,
            Self::Down | Self::DownSkip | Self::DownSkipExtras | Self::DownExact => 1,
            Self::Up(n) | Self::UpSkipTrivia(n) | Self::UpSkipExtras(n) | Self::UpExact(n) => {
                -(n as i32)
            }
        }
    }

    /// Pack an Up command: family flag, 2-bit mode, 5-bit level.
    fn up_byte(mode: u8, level: u8) -> u8 {
        assert!(
            (1..=Self::MAX_UP_LEVEL).contains(&level),
            "Up level overflow: {level} > {}",
            Self::MAX_UP_LEVEL
        );
        UP_FLAG | (mode << UP_MODE_SHIFT) | level
    }

    /// The level of this `Up*` nav, or `None` for any non-`Up*` nav.
    pub fn up_level(self) -> Option<u8> {
        match self {
            Self::Up(n) | Self::UpSkipTrivia(n) | Self::UpSkipExtras(n) | Self::UpExact(n) => {
                Some(n)
            }
            _ => None,
        }
    }

    /// The 2-bit Up mode tag (bits 6-5 of the encoded byte), or `None` for any
    /// non-`Up*` nav — the constraint family, independent of level.
    pub fn up_mode_tag(self) -> Option<u8> {
        let tag = match self {
            Self::Up(_) => UP_ANY,
            Self::UpSkipTrivia(_) => UP_SKIP_TRIVIA,
            Self::UpSkipExtras(_) => UP_SKIP_EXTRAS,
            Self::UpExact(_) => UP_EXACT,
            _ => return None,
        };
        Some(tag)
    }

    /// Whether `self` and `other` are the same `Up*` mode, ignoring level. False
    /// if either is not an `Up*` nav.
    pub fn same_up_mode(self, other: Self) -> bool {
        matches!(
            (self.up_mode_tag(), other.up_mode_tag()),
            (Some(a), Some(b)) if a == b
        )
    }

    /// This `Up*` nav with a different level, preserving its mode.
    ///
    /// Panics if `self` is not an `Up*` nav; callers gate on [`Nav::up_level`].
    pub fn with_up_level(self, level: u8) -> Self {
        match self {
            Self::Up(_) => Self::Up(level),
            Self::UpSkipTrivia(_) => Self::UpSkipTrivia(level),
            Self::UpSkipExtras(_) => Self::UpSkipExtras(level),
            Self::UpExact(_) => Self::UpExact(level),
            _ => panic!("with_up_level on non-Up nav: {self:?}"),
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

    /// Alternation branches match at their exact cursor position; the search
    /// among positions is owned by the parent (quantifier skip-retry, sequence
    /// advancement). Strips the search loop from any nav variant.
    pub fn to_exact(self) -> Self {
        match self {
            Self::Epsilon => Self::Epsilon,
            Self::Down | Self::DownSkip | Self::DownSkipExtras => Self::DownExact,
            Self::Next | Self::NextSkip | Self::NextSkipExtras => Self::NextExact,
            Self::Stay => Self::StayExact,
            Self::Up(n) | Self::UpSkipTrivia(n) | Self::UpSkipExtras(n) => Self::UpExact(n),
            Self::DownExact | Self::NextExact | Self::StayExact | Self::UpExact(_) => self,
        }
    }
}
