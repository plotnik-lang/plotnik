//! Syntax kinds for the query language.
//!
//! `SyntaxKind` serves dual roles: token kinds (from lexer) and node kinds (from parser).
//! Logos derives token recognition; node kinds lack token/regex attributes.
//! `QLang` implements Rowan's `Language` trait for tree construction.

#![allow(dead_code)] // Some items are for future use

use logos::Logos;
use rowan::Language;

/// All token and node kinds. Tokens first, then nodes, then `__LAST` sentinel.
/// `#[repr(u16)]` enables safe transmute in `kind_from_raw`.
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

    /// `::` for type annotations. Defined before `Colon` for correct precedence.
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

    /// Non-greedy `*?` quantifier
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

    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    #[regex(r"'(?:[^'\\]|\\.)*'")]
    #[doc(hidden)]
    StringLiteral, // Lexer-internal only

    DoubleQuote,
    SingleQuote,
    /// String content between quotes
    StrVal,

    #[token("ERROR")]
    KwError,

    #[token("MISSING")]
    KwMissing,

    #[token("pub")]
    Pub,

    /// Identifier. Accepts dots/hyphens for tree-sitter compat; parser validates per context.
    /// Defined after keywords so they take precedence.
    #[regex(r"[a-zA-Z][a-zA-Z0-9_.\-]*")]
    Id,

    #[token(".")]
    Dot,

    #[token("@")]
    At,

    #[regex(r"[ \t]+")]
    Whitespace,

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    #[regex(r"//[^\n]*", allow_greedy = true)]
    LineComment,

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment,

    /// XML-like tags matched as errors (common LLM output)
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*(?:\s+[^>]*)?>")]
    #[regex(r"</[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*>")]
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*/\s*>")]
    XMLGarbage,
    /// Tree-sitter predicates (unsupported)
    #[regex(r"#[a-zA-Z_][a-zA-Z0-9_]*[?!]?")]
    Predicate,
    /// Coalesced unrecognized characters
    Garbage,
    Error,

    // --- Node kinds (non-terminals) ---
    Root,
    Tree,
    Ref,
    Str,
    Field,
    Capture,
    Type,
    Quantifier,
    Seq,
    Alt,
    Branch,
    Wildcard,
    Anchor,
    NegatedField,
    Def,

    // Must be last - used for bounds checking in `kind_from_raw`
    #[doc(hidden)]
    __LAST,
}

use SyntaxKind::*;

impl SyntaxKind {
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(self, Whitespace | Newline | LineComment | BlockComment)
    }

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

/// Language tag for Rowan's tree types.
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

/// 64-bit bitset of `SyntaxKind`s for O(1) membership testing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenSet(u64);

impl TokenSet {
    /// Creates an empty token set.
    pub const EMPTY: TokenSet = TokenSet(0);

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

    #[inline]
    pub const fn single(kind: SyntaxKind) -> Self {
        let kind = kind as u16;
        assert!(kind < 64, "SyntaxKind value exceeds TokenSet capacity");
        TokenSet(1 << kind)
    }

    #[inline]
    pub const fn contains(&self, kind: SyntaxKind) -> bool {
        let kind = kind as u16;
        if kind >= 64 {
            return false;
        }
        self.0 & (1 << kind) != 0
    }

    #[inline]
    pub const fn union(self, other: TokenSet) -> TokenSet {
        TokenSet(self.0 | other.0)
    }
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_set();
        for i in 0..64u16 {
            if self.0 & (1 << i) != 0 && i < __LAST as u16 {
                let kind: SyntaxKind = unsafe { std::mem::transmute(i) };
                list.entry(&kind);
            }
        }
        list.finish()
    }
}

/// Pre-defined token sets for the parser.
pub mod token_sets {
    use super::*;

    /// FIRST set of expr. `At` excluded (captures wrap, not start).
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

    /// FIRST set for root-level expressions. Excludes `Dot`/`Negation` (tree-internal).
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

    pub const QUANTIFIERS: TokenSet = TokenSet::new(&[
        Star,
        Plus,
        Question,
        StarQuestion,
        PlusQuestion,
        QuestionQuestion,
    ]);

    pub const TRIVIA: TokenSet = TokenSet::new(&[Whitespace, Newline, LineComment, BlockComment]);
    pub const SEPARATORS: TokenSet = TokenSet::new(&[Comma, Pipe]);

    pub const TREE_RECOVERY: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen]);

    pub const ALT_RECOVERY: TokenSet = TokenSet::new(&[ParenClose]);

    pub const FIELD_RECOVERY: TokenSet =
        TokenSet::new(&[ParenClose, BracketClose, BraceClose, At, Colon]);

    pub const ROOT_RECOVERY: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen, Id]);

    pub const DEF_RECOVERY: TokenSet =
        TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen, Id, Equals]);

    pub const SEQ_RECOVERY: TokenSet = TokenSet::new(&[BraceClose, ParenClose, BracketClose]);
}
