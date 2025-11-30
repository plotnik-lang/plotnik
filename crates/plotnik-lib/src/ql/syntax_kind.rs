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
//!
//! Logos is derived directly on this enum; node kinds simply lack token/regex attributes.

#![allow(dead_code)] // Some items are for future use

use logos::Logos;
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
#[derive(Logos, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub enum SyntaxKind {
    #[token("(")]
    ParenOpen = 0,

    #[token(")")]
    ParenClose,

    #[token("[")]
    BracketOpen,

    #[token("]")]
    BracketClose,

    #[token(":")]
    Colon,

    #[token("=")]
    Equals,

    #[token("!")]
    Negation,

    #[token("~")]
    Tilde,

    #[token("_")]
    Underscore,

    #[token("*")]
    Star,

    #[token("+")]
    Plus,

    #[token("?")]
    Question,

    /// Non-greedy `*?` quantifier (matches minimum repetitions)
    #[token("*?")]
    StarQuestion,

    /// Non-greedy `+?` quantifier
    #[token("+?")]
    PlusQuestion,

    /// Non-greedy `??` quantifier
    #[token("??")]
    QuestionQuestion,

    /// Double-quoted string with backslash escapes
    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    StringLit,

    /// PascalCase identifier (e.g., `Foo`, `Bar`)
    #[regex(r"[A-Z][A-Za-z0-9]*")]
    UpperIdent,

    /// snake_case identifier (e.g., `identifier`, `function_definition`). Also used for capture names.
    #[regex(r"[a-z][a-z0-9_]*")]
    LowerIdent,

    #[token(".")]
    Dot,

    #[token("@")]
    At,

    /// Horizontal whitespace (spaces, tabs)
    #[regex(r"[ \t]+")]
    Whitespace,

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    #[regex(r"//[^\n]*")]
    LineComment,

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment,

    /// XML-like tags explicitly matched as errors (common LLM mistake)
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*(?:\s+[^>]*)?>")]
    #[regex(r"</[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*>")]
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*/\s*>")]
    UnexpectedXML,
    /// Consecutive unrecognized characters coalesced into one token
    UnexpectedFragment,
    /// Generic error token
    Error,

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
        matches!(self, Error | UnexpectedXML | UnexpectedFragment)
    }

    /// Returns a human-readable name for use in error messages.
    pub fn human_name(self) -> &'static str {
        match self {
            ParenOpen => "'('",
            ParenClose => "')'",
            BracketOpen => "'['",
            BracketClose => "']'",
            Colon => "':'",
            Equals => "'='",
            Negation => "'!'",
            Tilde => "'~'",
            Underscore => "'_' (wildcard)",
            Star => "'*'",
            Plus => "'+'",
            Question => "'?'",
            StarQuestion => "'*?' (non-greedy)",
            PlusQuestion => "'+?' (non-greedy)",
            QuestionQuestion => "'??' (non-greedy)",
            StringLit => "string literal",
            UpperIdent => "type name",
            LowerIdent => "identifier",
            Dot => "'.' (anchor)",
            At => "'@'",
            Whitespace | Newline => "whitespace",
            LineComment | BlockComment => "comment",
            UnexpectedXML => "unexpected XML",
            UnexpectedFragment => "unexpected characters",
            Error => "error",
            Root => "query",
            Pattern => "pattern",
            NamedNode => "node pattern",
            AnonNode => "anonymous node",
            Field => "field",
            Capture => "capture",
            Quantifier => "quantifier",
            Group => "group",
            Alternation => "alternation",
            Wildcard => "wildcard",
            Anchor => "anchor",
            NegatedField => "negated field",
            __LAST => "unknown",
        }
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

    pub const NAMED_NODE_RECOVERY: TokenSet =
        TokenSet::new(&[ParenOpen, BracketOpen, At]);

    pub const ALTERNATION_RECOVERY: TokenSet = TokenSet::new(&[ParenClose, At]);

    pub const FIELD_RECOVERY: TokenSet =
        TokenSet::new(&[ParenClose, BracketClose, At, Colon]);

    pub const ROOT_RECOVERY: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen]);
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
