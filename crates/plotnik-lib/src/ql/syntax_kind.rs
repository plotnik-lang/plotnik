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

    #[token("{")]
    BraceOpen,

    #[token("}")]
    BraceClose,

    /// Double colon for type annotations: `@name :: Type`
    /// Must be defined before single Colon for correct precedence
    #[token("::")]
    DoubleColon,

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

    /// Slash for supertype paths: `(expression/binary_expression)`
    #[token("/")]
    Slash,

    /// Comma (invalid separator, for error recovery)
    #[token(",")]
    Comma,

    /// Pipe (invalid separator, for error recovery)
    #[token("|")]
    Pipe,

    /// String literal (double or single quoted) - split by lexer post-processing
    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    #[regex(r"'(?:[^'\\]|\\.)*'")]
    StringLiteral,

    /// Double quote character (from string literal splitting)
    DoubleQuote,

    /// Single quote character (from string literal splitting)
    SingleQuote,

    /// String content between quotes (from string literal splitting)
    StrVal,

    /// ERROR keyword for matching parser error nodes
    #[token("ERROR")]
    KwError,

    /// MISSING keyword for matching error recovery nodes
    #[token("MISSING")]
    KwMissing,

    /// Loose identifier for all naming contexts (definitions, fields, node types, etc.)
    /// Accepts dots and hyphens for tree-sitter compatibility; parser validates per context.
    /// Defined after KwError/KwMissing so keywords take precedence.
    #[regex(r"[a-zA-Z][a-zA-Z0-9_.\-]*")]
    Id,

    #[token(".")]
    Dot,

    /// At sign for captures: `@`
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
    XMLGarbage,
    /// Tree-sitter predicate syntax (unsupported, for clear error messages)
    /// Matches #eq?, #match?, #set!, #is?, etc.
    #[regex(r"#[a-zA-Z_][a-zA-Z0-9_]*[?!]?")]
    Predicate,
    /// Consecutive unrecognized characters coalesced into one token
    Garbage,
    /// Generic error token
    Error,

    /// Root node containing the entire query
    Root,
    /// Tree expression: `(type children...)`, `(_)`, `(ERROR)`, `(MISSING ...)`
    Tree,
    /// Reference to user-defined expression: `(Expr)` where Expr is PascalCase
    Ref,
    /// Literal/anonymous node: `"keyword"` (legacy, use Str)
    Lit,
    /// String literal node containing quote tokens and content
    Str,
    /// Field specification: `name: expr`
    Field,
    /// Capture wrapping an expression: `(expr) @name` or `(expr) @name :: Type`
    Capture,
    /// Type annotation: `::Type` after a capture
    Type,
    /// Quantifier wrapping an expression, e.g., `(expr)*` becomes `Quantifier { Tree, Star }`
    Quantifier,
    /// Sibling sequence: `{expr1 expr2 ...}`
    Seq,
    /// Choice between alternatives: `[a b c]`
    Alt,
    /// Branch in a tagged alternation: `Label: expr`
    Branch,
    /// Wildcard: `_` matches any node
    Wildcard,
    /// Anchor: `.` constrains position relative to siblings
    Anchor,
    /// Negated field assertion: `!field` asserts field is absent
    NegatedField,
    /// Named expression definition: `Name = expr`
    Def,

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
        matches!(self, Error | XMLGarbage | Garbage | Predicate)
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

    /// Tokens that can start an expression (FIRST set of the expression production).
    /// Note: At is not included because captures wrap expressions, they don't start them.
    pub const EXPR_FIRST: TokenSet = TokenSet::new(&[
        ParenOpen,
        BracketOpen,
        BraceOpen,
        Underscore,
        Id,
        DoubleQuote,
        SingleQuote,
        Dot,
        Negation,
        KwError,
        KwMissing,
    ]);

    /// Tokens that can start a valid expression at root level (anonymous definition).
    /// Excludes bare Id (only valid as node type inside parens), Dot (anchor),
    /// and Negation (negated field) which only make sense inside tree context.
    pub const ROOT_EXPR_FIRST: TokenSet = TokenSet::new(&[
        ParenOpen,
        BracketOpen,
        BraceOpen,
        Underscore,
        Id,
        DoubleQuote,
        SingleQuote,
        KwError,
        KwMissing,
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

    /// Invalid separator tokens (comma, pipe) - for error recovery
    pub const SEPARATORS: TokenSet = TokenSet::new(&[Comma, Pipe]);

    pub const TREE_RECOVERY: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen]);

    pub const ALT_RECOVERY: TokenSet = TokenSet::new(&[ParenClose]);

    pub const FIELD_RECOVERY: TokenSet =
        TokenSet::new(&[ParenClose, BracketClose, BraceClose, At, Colon]);

    pub const ROOT_RECOVERY: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen, Id]);

    /// Recovery set for named definitions (Name = ...)
    pub const DEF_RECOVERY: TokenSet =
        TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen, Id, Equals]);

    pub const SEQ_RECOVERY: TokenSet = TokenSet::new(&[BraceClose, ParenClose, BracketClose]);
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

    #[test]
    fn test_syntax_kind_count_under_64() {
        // Ensure we don't exceed TokenSet capacity
        assert!(
            (__LAST as u16) < 64,
            "SyntaxKind has {} variants, exceeds TokenSet capacity of 64",
            __LAST as u16
        );
    }
}
