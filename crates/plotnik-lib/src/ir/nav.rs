//! Tree navigation instructions for query execution.
//!
//! Navigation decisions are resolved at graph construction time, not runtime.
//! Each transition carries its own `Nav` instruction.

/// Navigation instruction determining cursor movement and skip policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct Nav {
    pub kind: NavKind,
    /// Ascent level count for `Up*` variants, ignored otherwise.
    pub level: u8,
}

impl Nav {
    pub const fn stay() -> Self {
        Self {
            kind: NavKind::Stay,
            level: 0,
        }
    }

    pub const fn next() -> Self {
        Self {
            kind: NavKind::Next,
            level: 0,
        }
    }

    pub const fn next_skip_trivia() -> Self {
        Self {
            kind: NavKind::NextSkipTrivia,
            level: 0,
        }
    }

    pub const fn next_exact() -> Self {
        Self {
            kind: NavKind::NextExact,
            level: 0,
        }
    }

    pub const fn down() -> Self {
        Self {
            kind: NavKind::Down,
            level: 0,
        }
    }

    pub const fn down_skip_trivia() -> Self {
        Self {
            kind: NavKind::DownSkipTrivia,
            level: 0,
        }
    }

    pub const fn down_exact() -> Self {
        Self {
            kind: NavKind::DownExact,
            level: 0,
        }
    }

    pub const fn up(level: u8) -> Self {
        Self {
            kind: NavKind::Up,
            level,
        }
    }

    pub const fn up_skip_trivia(level: u8) -> Self {
        Self {
            kind: NavKind::UpSkipTrivia,
            level,
        }
    }

    pub const fn up_exact(level: u8) -> Self {
        Self {
            kind: NavKind::UpExact,
            level,
        }
    }

    /// Returns true if this is a Stay navigation (no movement).
    #[inline]
    pub const fn is_stay(&self) -> bool {
        matches!(self.kind, NavKind::Stay)
    }

    /// Returns true if this is a horizontal sibling traversal (Next*).
    #[inline]
    pub const fn is_next(&self) -> bool {
        matches!(
            self.kind,
            NavKind::Next | NavKind::NextSkipTrivia | NavKind::NextExact
        )
    }

    /// Returns true if this descends into children (Down*).
    #[inline]
    pub const fn is_down(&self) -> bool {
        matches!(
            self.kind,
            NavKind::Down | NavKind::DownSkipTrivia | NavKind::DownExact
        )
    }

    /// Returns true if this ascends to parent(s) (Up*).
    #[inline]
    pub const fn is_up(&self) -> bool {
        matches!(
            self.kind,
            NavKind::Up | NavKind::UpSkipTrivia | NavKind::UpExact
        )
    }

    /// Returns true if this navigation skips only trivia nodes.
    #[inline]
    pub const fn is_skip_trivia(&self) -> bool {
        matches!(
            self.kind,
            NavKind::NextSkipTrivia | NavKind::DownSkipTrivia | NavKind::UpSkipTrivia
        )
    }

    /// Returns true if this navigation requires exact position (no skipping).
    #[inline]
    pub const fn is_exact(&self) -> bool {
        matches!(
            self.kind,
            NavKind::NextExact | NavKind::DownExact | NavKind::UpExact
        )
    }
}

/// Navigation kind determining movement direction and skip policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NavKind {
    /// No movement. Used only for first transition when cursor is at root.
    Stay = 0,

    // Sibling traversal (horizontal)
    /// Skip any nodes to find match.
    Next = 1,
    /// Skip trivia only, fail if non-trivia skipped.
    NextSkipTrivia = 2,
    /// No skipping, current sibling must match.
    NextExact = 3,

    // Enter children (descend)
    /// Skip any among children.
    Down = 4,
    /// Skip trivia only among children.
    DownSkipTrivia = 5,
    /// First child must match, no skip.
    DownExact = 6,

    // Exit children (ascend)
    /// Ascend `level` levels, no constraint.
    Up = 7,
    /// Validate last non-trivia, ascend `level` levels.
    UpSkipTrivia = 8,
    /// Validate last child, ascend `level` levels.
    UpExact = 9,
}
