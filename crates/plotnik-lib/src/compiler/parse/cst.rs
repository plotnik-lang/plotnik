//! Syntax kinds for the query language.
//!
//! `SyntaxKind` serves dual roles: token kinds (from lexer) and node kinds (from parser).
//! Logos derives token recognition; node kinds lack token/regex attributes.
//! `QueryLang` implements Rowan's `Language` trait for tree construction.

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

    /// `::` for capture types. Longest match wins over `Colon`.
    #[token("::")]
    DoubleColon,

    #[token(":")]
    Colon,

    #[token("=")]
    Equals,

    #[token("!")]
    Negation,

    #[token("-")]
    Minus,

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

    /// Lazy `*?` quantifier.
    #[token("*?")]
    StarQuestion,

    /// Lazy `+?` quantifier.
    #[token("+?")]
    PlusQuestion,

    /// Lazy `??` quantifier.
    #[token("??")]
    QuestionQuestion,

    /// Slash for tree-sitter supertype paths: `(expression/binary_expression)`
    #[token("/")]
    Slash,

    /// Hash for native category syntax: `(expression#)` and `(expression#binary_expression)`.
    /// Always a bare `#`; the subtype after it is a normal `Id`, so `#sub` and `/sub` share
    /// one grammar. A misplaced `#` (e.g. a tree-sitter `#eq?` predicate) is diagnosed by the
    /// parser, not the lexer.
    #[token("#")]
    Hash,

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

    /// String with no closing quote before end of line. Closed strings always
    /// match longer, so this only wins when no closing quote exists.
    #[regex(r#""(?:[^"\\\n]|\\[^\n])*"#)]
    #[regex(r"'(?:[^'\\\n]|\\[^\n])*")]
    UnterminatedString,

    DoubleQuote,
    SingleQuote,
    StringContent,

    #[token("ERROR")]
    KwError,

    #[token("MISSING")]
    KwMissing,

    /// Identifier. Accepts dots/hyphens for tree-sitter compat; parser validates per context.
    /// Keywords win over this regex via logos's higher literal-token priority.
    #[regex(r"[a-zA-Z][a-zA-Z0-9_.\-]*")]
    Id,

    #[token(".!")]
    DotBang,

    #[token(".")]
    Dot,

    /// Regular capture: @name
    #[regex(r"@[a-zA-Z][a-zA-Z0-9_]*")]
    CaptureToken,

    /// Suppressive capture: @_ or @_name
    #[regex(r"@_[a-zA-Z0-9_]*")]
    SuppressiveCapture,

    /// Bare @ (for error recovery: "capture without target")
    #[token("@")]
    At,

    #[regex(r"[ \t]+")]
    Whitespace,

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    #[regex(r"//[^\n]*", allow_greedy = true)]
    #[regex(r";[^\n]*", allow_greedy = true)]
    LineComment,

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment,

    /// Shebang line: `#!/usr/bin/env -S plotnik run -l typescript`.
    /// Trivia only at offset 0; the lexer downgrades mid-file matches to `Garbage`.
    #[regex(r"#![^\n]*", allow_greedy = true)]
    Shebang,

    #[token("==")]
    OpEq,

    #[token("!=")]
    OpNe,

    #[token("^=")]
    OpStartsWith,

    #[token("$=")]
    OpEndsWith,

    /// Longest match wins over `Star`.
    #[token("*=")]
    OpContains,

    /// `=~` for predicate regex match (when followed by string or error)
    #[token("=~")]
    OpRegexMatch,

    /// `!~` for predicate regex no-match (when followed by string or error)
    #[token("!~")]
    OpRegexNoMatch,

    /// `=~` followed by regex literal: `=~ /pattern/`
    /// Compound token to avoid `//` being lexed as line comment.
    #[regex(r"=~[ \t\r\n]*/", lex_regex_predicate)]
    RegexPredicateMatch,

    /// `!~` followed by regex literal: `!~ /pattern/`
    #[regex(r"!~[ \t\r\n]*/", lex_regex_predicate)]
    RegexPredicateNoMatch,

    /// Regex literal token (after splitting compound predicate)
    RegexLiteral,

    /// Coalesced unrecognized characters
    Garbage,
    Error,

    Root,
    NamedNode,
    DefRef,
    Str,
    Field,
    Capture,
    CaptureType,
    Quantifier,
    Sequence,
    Alternation,
    Alternative,
    Wildcard,
    Anchor,
    NegatedField,
    Def,
    /// Predicate on a node: `(identifier == "foo")`
    NodePredicate,
    Regex,

    // Must be last - used for bounds checking in `kind_from_raw`
    #[doc(hidden)]
    __LAST,
}

use SyntaxKind::*;

/// Logos callback for regex predicate tokens.
/// Called after matching `=~\s*/` or `!~\s*/`, consumes until closing unescaped `/`.
fn lex_regex_predicate(lexer: &mut logos::Lexer<SyntaxKind>) -> bool {
    let remaining = lexer.remainder();
    let mut backslash_count = 0;

    let mut line_end = remaining.len();
    for (i, c) in remaining.char_indices() {
        if c == '/' && backslash_count % 2 == 0 {
            lexer.bump(i + 1);
            return true;
        }
        if c == '\n' {
            line_end = i;
            break;
        }
        backslash_count = if c == '\\' { backslash_count + 1 } else { 0 };
    }

    // No closing slash on this line: stop at the line end so the rest of the
    // file still lexes (an unclosed regex must not swallow every following
    // definition); the parser reports the unclosed literal.
    lexer.bump(line_end);
    true
}

impl SyntaxKind {
    #[inline]
    pub fn is_trivia(self) -> bool {
        matches!(
            self,
            Whitespace | Newline | LineComment | BlockComment | Shebang
        )
    }

    #[inline]
    pub fn is_error(self) -> bool {
        matches!(self, Error | Garbage)
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
pub enum QueryLang {}

impl Language for QueryLang {
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

pub type SyntaxNode = rowan::SyntaxNode<QueryLang>;
pub type SyntaxToken = rowan::SyntaxToken<QueryLang>;
