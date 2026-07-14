//! Navigation command encoding for bytecode instructions.
//!
//! Navigation determines how the VM moves through the Tree-sitter syntax tree.

use crate::SkipClass;

/// What a sibling search may skip while looking for (or retrying past) a
/// candidate. Derived from the instruction's [`Nav`] via [`Nav::skip_policy`];
/// the engine consults it both when scanning forward to a first candidate and
/// when resuming a search past a failed one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipPolicy {
    /// Skip any nodes until match.
    Any,
    /// Skip trivia only (fail if non-trivia must be skipped).
    Trivia,
    /// Skip tree-sitter extras only (fail if a regular anonymous token must be skipped).
    Extras,
    /// No skipping allowed (exact match required).
    Exact,
}

impl SkipPolicy {
    /// The [`SkipClass`] of nodes this policy may skip.
    pub fn skip_class(self) -> SkipClass {
        match self {
            Self::Any => SkipClass::Any,
            Self::Trivia => SkipClass::Trivia,
            Self::Extras => SkipClass::Extras,
            Self::Exact => SkipClass::Exact,
        }
    }
}

/// Navigation command for VM execution.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default)]
pub enum Nav {
    /// Epsilon transition: pure control flow, no cursor movement or node check.
    /// Used for forks, quantifier loops, and effect-only transitions.
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
    /// Assert the current node has no children beyond trivia, without moving.
    ///
    /// The `Childless*` family is the empty-match arm of a leading or trailing
    /// anchor: when a node's whole child list matches empty, the cursor
    /// never descends, so no `Down*` entry or `Up*` ascent carries the
    /// anchor's check — either one degrades to "the node has no children the
    /// anchor's skip policy would reject".
    ChildlessSkipTrivia,
    /// Assert the current node has no children beyond extras, without moving.
    ChildlessSkipExtras,
    /// Assert the current node has no children at all, without moving.
    ChildlessExact,
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
    /// leaves (see `CursorWrapper::go_up`).
    pub const MAX_UP_LEVEL: u8 = (1 << UP_MODE_SHIFT) - 1;

    /// Decode from a bytecode byte, panicking on an invalid encoding.
    ///
    /// Byte layout:
    /// - Bit 7 set — an Up command: bits 6-5 are the mode (`00` Up, `01`
    ///   UpSkipTrivia, `10` UpSkipExtras, `11` UpExact), bits 4-0 the level
    ///   (`1..=31`).
    /// - Bit 7 clear — a standard command: bits 6-0 are its enum value (`0..=13`).
    pub fn from_byte(b: u8) -> Self {
        Self::try_from_byte(b).unwrap_or_else(|| panic!("invalid nav byte: {b:#04x}"))
    }

    /// Non-panicking nav decode, for validating an untrusted instruction stream
    /// at load time before the VM decodes it. The invalid encodings are an Up
    /// byte with a zero level and a standard byte whose enum value is unassigned
    /// (`14..=127`).
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
            11 => Self::ChildlessSkipTrivia,
            12 => Self::ChildlessSkipExtras,
            13 => Self::ChildlessExact,
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
            Self::ChildlessSkipTrivia => 11,
            Self::ChildlessSkipExtras => 12,
            Self::ChildlessExact => 13,
            Self::Up(n) => Self::up_byte(UP_ANY, n),
            Self::UpSkipTrivia(n) => Self::up_byte(UP_SKIP_TRIVIA, n),
            Self::UpSkipExtras(n) => Self::up_byte(UP_SKIP_EXTRAS, n),
            Self::UpExact(n) => Self::up_byte(UP_EXACT, n),
        }
    }

    /// Cursor-depth delta this navigation applies to the abstract tree cursor.
    ///
    /// The verifier only tracks depth, not sibling position: `Down*` enters a
    /// child, `Up*` exits one or more parents, and stay/next/epsilon variants
    /// keep the current depth.
    pub const fn depth_delta(self) -> i32 {
        match self {
            Self::Epsilon
            | Self::Stay
            | Self::StayExact
            | Self::Next
            | Self::NextSkip
            | Self::NextSkipExtras
            | Self::NextExact
            | Self::ChildlessSkipTrivia
            | Self::ChildlessSkipExtras
            | Self::ChildlessExact => 0,
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

    /// The skip policy this nav's match attempt runs under — the single source
    /// of truth for what a search may skip (`CursorWrapper::navigate`
    /// returns it after moving; backtrack resume re-derives it from the
    /// re-decoded instruction).
    ///
    /// Non-searching navs (`Stay`, `Up*`) report `Any`: their match runs at a
    /// position chosen by someone else, so the policy is only consulted if the
    /// constraint fails and the engine wanders — pre-existing semantics kept
    /// as-is. `Childless*` and the `*Exact` family report `Exact`: they have
    /// exactly one candidate.
    pub fn skip_policy(self) -> SkipPolicy {
        match self {
            Self::Epsilon | Self::Stay | Self::Next | Self::Down => SkipPolicy::Any,
            Self::NextSkip | Self::DownSkip => SkipPolicy::Trivia,
            Self::NextSkipExtras | Self::DownSkipExtras => SkipPolicy::Extras,
            Self::StayExact
            | Self::NextExact
            | Self::DownExact
            | Self::ChildlessSkipTrivia
            | Self::ChildlessSkipExtras
            | Self::ChildlessExact => SkipPolicy::Exact,
            Self::Up(_) | Self::UpSkipTrivia(_) | Self::UpSkipExtras(_) | Self::UpExact(_) => {
                SkipPolicy::Any
            }
        }
    }

    /// Whether this nav performs a sibling search the *engine* owns: it moves
    /// to a first candidate (`Down*`/`Next*`) and may advance past rejected ones
    /// per its skip policy. Acceptance at such a position is a choice point —
    /// the engine leaves a resume checkpoint so a later failure retries the
    /// search from the next admissible candidate.
    ///
    /// The `*Exact` members navigate but have a single candidate, and
    /// `Stay`/`StayExact` matches run at positions owned by an outer search
    /// (a position-search loop, a Call's retry checkpoint) — none of them are
    /// engine-owned searches. Lowering upholds the complement: every navigation state
    /// internal to an NFA-level retry loop is emitted exact
    /// (`emit_wildcard_nav`), so a search always has exactly one retry owner.
    pub fn is_sibling_search(self) -> bool {
        matches!(
            self,
            Self::Down
                | Self::DownSkip
                | Self::DownSkipExtras
                | Self::Next
                | Self::NextSkip
                | Self::NextSkipExtras
        )
    }

    /// Navigation that advances to the next sibling while preserving this nav's
    /// skip policy.
    ///
    /// Shared source of truth for the two places that advance a sibling search
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
            | Self::ChildlessSkipTrivia
            | Self::ChildlessSkipExtras
            | Self::ChildlessExact
            | Self::Up(_)
            | Self::UpSkipTrivia(_)
            | Self::UpSkipExtras(_)
            | Self::UpExact(_) => Self::Next,
        }
    }

    /// Alternation alternatives match at their exact cursor position; the search
    /// among positions is owned by the parent (quantifier skip-retry, sequence
    /// advancement). Strips the search loop from any nav variant.
    pub fn to_exact(self) -> Self {
        match self {
            Self::Epsilon => Self::Epsilon,
            Self::Down | Self::DownSkip | Self::DownSkipExtras => Self::DownExact,
            Self::Next | Self::NextSkip | Self::NextSkipExtras => Self::NextExact,
            Self::Stay => Self::StayExact,
            Self::Up(n) | Self::UpSkipTrivia(n) | Self::UpSkipExtras(n) => Self::UpExact(n),
            // Childless asserts at the current position with no search to strip;
            // its skip class is the assertion itself, not a search policy.
            Self::DownExact
            | Self::NextExact
            | Self::StayExact
            | Self::UpExact(_)
            | Self::ChildlessSkipTrivia
            | Self::ChildlessSkipExtras
            | Self::ChildlessExact => self,
        }
    }
}
