//! Syntax kinds for the query language.
//!
//! This module defines all token and node kinds used in the syntax tree,
//! along with a `TokenSet` bitset for efficient membership testing in the parser.
//!
//! ## Architecture
//!
//! The `SyntaxKind` enum has a dual role:
//! - Token kinds (terminals): produced by the lexer, represent atomic text spans
//! - Node kinds (non-terminals): created by the parser, represent composite structures
//!
//! Rowan requires a `Language` trait implementation to convert between our `SyntaxKind`
//! and its internal `rowan::SyntaxKind` (a newtype over `u16`). That's what `QLang` provides.

#![allow(dead_code)] // Some items are for future use

use rowan::Language;

/// All kinds of tokens and nodes in the syntax tree.
///
/// ## Layout
///
/// Variants are ordered: tokens first, then nodes, then `__LAST` sentinel.
/// The `#[repr(u16)]` ensures we can safely transmute from the discriminant.
///
/// ## Token vs Node distinction
///
/// The parser only ever builds nodes; tokens come from the lexer.
/// A token's text is sliced from source on demand via its span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    // =========================
    // Tokens (terminal symbols)
    // =========================
    ParenOpen = 0,
    ParenClose,
    BracketOpen,
    BracketClose,
    Colon,
    Equals,
    Negation,
    Tilde,
    Underscore,
    Star,
    Plus,
    Question,
    /// Non-greedy `*?` quantifier (matches minimum repetitions)
    StarQuestion,
    /// Non-greedy `+?` quantifier
    PlusQuestion,
    /// Non-greedy `??` quantifier
    QuestionQuestion,
    StringLit,
    /// PascalCase identifier (e.g., `Foo`, `Bar`)
    UpperIdent,
    /// snake_case identifier (e.g., `identifier`, `function_definition`)
    LowerIdent,
    Dot,
    At,
    /// Capture name after `@`, may include dots (e.g., in `@var.field` this is `var.field`)
    CaptureName,
    Hash,

    // Trivia tokens
    Whitespace,
    Newline,
    LineComment,
    BlockComment,

    // Error token
    Error,

    // ================================
    // Nodes (non-terminal symbols)
    // ================================
    /// Root node containing the entire query
    Root,
    /// A pattern matching a node (e.g., `(identifier)`)
    Pattern,
    /// Named node pattern: `(type children...)`
    NamedNode,
    /// Anonymous/literal node pattern: `"keyword"`
    AnonNode,
    /// Field specification: `name: pattern`
    Field,
    /// Capture binding: `@name` or `@name.field`
    Capture,
    /// Predicate call: `#match?`, `#eq?`, etc. (not yet implemented in parser)
    Predicate,
    /// Arguments to a predicate (not yet implemented)
    PredicateArgs,
    /// Quantifier wrapping a pattern, e.g., `(expr)*` becomes `Quantifier { NamedNode, Star }`
    Quantifier,
    /// Grouping of patterns
    Group,
    /// Choice between alternatives: `[a b c]`
    Alternation,
    /// Wildcard: `_` matches any node
    Wildcard,
    /// Anchor: `.` constrains position relative to siblings
    Anchor,
    /// Negated field assertion: `!field` asserts field is absent
    NegatedField,

    // Must be last - used for bounds checking in `kind_from_raw`
    #[doc(hidden)]
    __LAST,
}

use SyntaxKind::*;

impl SyntaxKind {
    /// Returns `true` if this is a trivia token (whitespace or comment).
    ///
    /// Trivia tokens are buffered during parsing and attached to the next node
    /// as leading trivia. This preserves formatting information in the CST.
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(self, Whitespace | Newline | LineComment | BlockComment)
    }

    /// Returns `true` if this is an error token.
    #[inline]
    pub fn is_error(self) -> bool {
        self == Error
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    #[inline]
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

/// Language tag for parameterizing Rowan's tree types.
///
/// This is a zero-sized enum (uninhabited) used purely as a type-level marker.
/// Rowan uses it to associate syntax trees with our `SyntaxKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum QLang {}

impl Language for QLang {
    type Kind = SyntaxKind;

    fn kind_from_raw(raw: rowan::SyntaxKind) -> Self::Kind {
        assert!(raw.0 < __LAST as u16);
        // SAFETY: We've verified the value is in bounds, and SyntaxKind is repr(u16)
        unsafe { std::mem::transmute::<u16, SyntaxKind>(raw.0) }
    }

    fn kind_to_raw(kind: Self::Kind) -> rowan::SyntaxKind {
        kind.into()
    }
}

/// Type aliases for Rowan types parameterized by our language.
pub type SyntaxNode = rowan::SyntaxNode<QLang>;
pub type SyntaxToken = rowan::SyntaxToken<QLang>;
pub type SyntaxElement = rowan::NodeOrToken<SyntaxNode, SyntaxToken>;

/// A set of `SyntaxKind`s implemented as a 64-bit bitset.
///
/// ## Usage
///
/// Used throughout the parser for O(1) membership testing of FIRST/FOLLOW/RECOVERY sets.
/// The limitation is 64 variants max, which is enforced by compile-time asserts in `new()`.
///
/// ## Construction
///
/// All constructors are `const fn`, so token sets can be defined as constants:
///
/// ```ignore
/// const DELIMITERS: TokenSet = TokenSet::new(&[ParenOpen, ParenClose, BracketOpen, BracketClose]);
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenSet(u64);

impl TokenSet {
    /// Creates an empty token set.
    pub const EMPTY: TokenSet = TokenSet(0);

    /// Creates a token set from a slice of kinds.
    ///
    /// Panics at compile time if any kind's discriminant >= 64.
    #[inline]
    pub const fn new(kinds: &[SyntaxKind]) -> Self {
        let mut bits = 0u64;
        let mut i = 0;
        while i < kinds.len() {
            let kind = kinds[i] as u16;
            assert!(kind < 64, "SyntaxKind value exceeds TokenSet capacity");
            bits |= 1 << kind;
            i += 1;
        }
        TokenSet(bits)
    }

    /// Creates a token set containing exactly one kind.
    #[inline]
    pub const fn single(kind: SyntaxKind) -> Self {
        let kind = kind as u16;
        assert!(kind < 64, "SyntaxKind value exceeds TokenSet capacity");
        TokenSet(1 << kind)
    }

    /// Returns `true` if the set contains the given kind.
    #[inline]
    pub const fn contains(&self, kind: SyntaxKind) -> bool {
        let kind = kind as u16;
        if kind >= 64 {
            return false;
        }
        self.0 & (1 << kind) != 0
    }

    /// Returns the union of two token sets.
    #[inline]
    pub const fn union(self, other: TokenSet) -> TokenSet {
        TokenSet(self.0 | other.0)
    }
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_set();
        for i in 0..64u16 {
            if self.0 & (1 << i) != 0 {
                if i < __LAST as u16 {
                    let kind: SyntaxKind = unsafe { std::mem::transmute(i) };
                    list.entry(&kind);
                }
            }
        }
        list.finish()
    }
}

/// Pre-defined token sets used throughout the parser.
///
/// ## Recovery sets
///
/// Recovery sets follow matklad's resilient parsing approach: when the parser
/// encounters an unexpected token, it consumes tokens until it finds one in
/// the recovery set (typically the FOLLOW set of ancestor productions).
/// This prevents cascading errors and allows parsing to continue.
pub mod token_sets {
    use super::*;

    /// Tokens that can start a pattern (FIRST set of the pattern production).
    pub const PATTERN_FIRST: TokenSet = TokenSet::new(&[
        ParenOpen,
        BracketOpen,
        Underscore,
        UpperIdent,
        LowerIdent,
        StringLit,
        At,
        Dot,
        Negation,
    ]);

    /// Quantifier tokens that can follow a pattern.
    pub const QUANTIFIERS: TokenSet = TokenSet::new(&[
        Star,
        Plus,
        Question,
        StarQuestion,
        PlusQuestion,
        QuestionQuestion,
    ]);

    /// Trivia tokens.
    pub const TRIVIA: TokenSet = TokenSet::new(&[Whitespace, Newline, LineComment, BlockComment]);

    // =========================================================================
    // RECOVERY sets
    //
    // When parsing fails inside a production, these sets determine when to
    // stop consuming error tokens and return control to the parent parser.
    // =========================================================================

    /// Recovery inside named node `(...)`.
    /// Includes tokens that could be siblings or parent constructs.
    pub const NAMED_NODE_RECOVERY: TokenSet = TokenSet::new(&[
        ParenOpen,   // sibling node
        BracketOpen, // alternation
        At,          // capture
        Hash,        // predicate
    ]);

    /// Recovery inside alternation `[...]`.
    pub const ALTERNATION_RECOVERY: TokenSet = TokenSet::new(&[
        ParenClose, // parent node
        At,         // capture
        Hash,       // predicate
    ]);

    /// Recovery inside field value `name: pattern`.
    pub const FIELD_RECOVERY: TokenSet = TokenSet::new(&[
        ParenClose,
        BracketClose,
        At,
        Hash,
        Colon, // next field
    ]);

    /// Recovery at top level.
    pub const ROOT_RECOVERY: TokenSet = TokenSet::new(&[
        ParenOpen,   // new pattern
        BracketOpen, // new alternation
        Hash,        // predicate
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_set_contains() {
        let set = TokenSet::new(&[ParenOpen, ParenClose, Star]);
        assert!(set.contains(ParenOpen));
        assert!(set.contains(ParenClose));
        assert!(set.contains(Star));
        assert!(!set.contains(Plus));
        assert!(!set.contains(Colon));
    }

    #[test]
    fn test_token_set_union() {
        let a = TokenSet::new(&[ParenOpen, ParenClose]);
        let b = TokenSet::new(&[Star, Plus]);
        let c = a.union(b);
        assert!(c.contains(ParenOpen));
        assert!(c.contains(ParenClose));
        assert!(c.contains(Star));
        assert!(c.contains(Plus));
        assert!(!c.contains(Colon));
    }

    #[test]
    fn test_token_set_single() {
        let set = TokenSet::single(Colon);
        assert!(set.contains(Colon));
        assert!(!set.contains(ParenOpen));
    }

    #[test]
    fn test_is_trivia() {
        assert!(Whitespace.is_trivia());
        assert!(Newline.is_trivia());
        assert!(LineComment.is_trivia());
        assert!(BlockComment.is_trivia());
        assert!(!ParenOpen.is_trivia());
        assert!(!Error.is_trivia());
    }
}