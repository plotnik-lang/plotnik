//! Parser-internal token sets (FIRST/recovery sets) over `SyntaxKind`.
//!
//! `SyntaxKind`, `QueryLang`, and the rowan tree types are shared data and live
//! in `plotnik-compiler-core`; they are re-exported here so the parser and its
//! consumers keep referring to them as `crate::parser::cst::*`. `TokenSet` and
//! the `token_sets` consts are parser recovery policy, not shared data, so they
//! stay with the parser.

use rowan::Language;

pub use plotnik_compiler_core::cst::{QueryLang, SyntaxKind, SyntaxNode, SyntaxToken};

use plotnik_compiler_core::cst::SyntaxKind::*;

/// 128-bit bitset of `SyntaxKind`s for O(1) membership testing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenSet(u128);

impl TokenSet {
    /// Panics at compile time if any kind's discriminant >= 128.
    #[inline]
    pub const fn new(kinds: &[SyntaxKind]) -> Self {
        let mut bits = 0u128;
        let mut i = 0;
        while i < kinds.len() {
            let kind = kinds[i] as u16;
            assert!(kind < 128, "SyntaxKind value exceeds TokenSet capacity");
            bits |= 1 << kind;
            i += 1;
        }
        TokenSet(bits)
    }

    #[inline]
    pub const fn contains(&self, kind: SyntaxKind) -> bool {
        let kind = kind as u16;
        if kind >= 128 {
            return false;
        }
        self.0 & (1 << kind) != 0
    }
}

impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut list = f.debug_set();
        for i in 0..128u16 {
            if self.0 & (1 << i) != 0 && i < __LAST as u16 {
                list.entry(&QueryLang::kind_from_raw(rowan::SyntaxKind(i)));
            }
        }
        list.finish()
    }
}

/// Pre-defined token sets for the parser.
pub mod token_sets {
    use super::*;

    /// FIRST set of pattern. `At` excluded (captures wrap, not start).
    /// `UnterminatedString` included so every expression position reports
    /// it as an unclosed string instead of a generic unexpected token.
    pub const EXPR_FIRST_TOKENS: TokenSet = TokenSet::new(&[
        ParenOpen,
        BracketOpen,
        BraceOpen,
        Underscore,
        Id,
        DoubleQuote,
        SingleQuote,
        UnterminatedString,
        DotBang,
        Dot,
        Negation,
        Minus,
        KwError,
        KwMissing,
    ]);

    /// FIRST set for root-level expressions. Excludes anchors/`Negation` (tree-internal).
    pub const ROOT_EXPR_FIRST_TOKENS: TokenSet = TokenSet::new(&[
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

    pub const SEPARATORS: TokenSet = TokenSet::new(&[Comma, Pipe]);

    pub const NODE_RECOVERY_TOKENS: TokenSet = TokenSet::new(&[ParenOpen, BracketOpen, BraceOpen]);

    pub const ALT_RECOVERY_TOKENS: TokenSet = TokenSet::new(&[ParenClose]);

    pub const SEQ_RECOVERY_TOKENS: TokenSet =
        TokenSet::new(&[BraceClose, ParenClose, BracketClose]);

    pub const PREDICATE_OPS: TokenSet = TokenSet::new(&[
        OpEq,
        OpNe,
        OpStartsWith,
        OpEndsWith,
        OpContains,
        OpRegexMatch,
        OpRegexNoMatch,
    ]);
}
