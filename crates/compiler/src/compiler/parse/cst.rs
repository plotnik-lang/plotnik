use logos::Logos;
use rowan::Language;

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

    #[token("_")]
    Underscore,

    #[token("*")]
    Star,

    #[token("+")]
    Plus,

    #[token("?")]
    Question,

    #[token("*?")]
    StarQuestion,

    #[token("+?")]
    PlusQuestion,

    #[token("??")]
    QuestionQuestion,

    #[token("/")]
    Slash,

    #[token(",")]
    Comma,

    #[token("|")]
    Pipe,

    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    #[regex(r"'(?:[^'\\]|\\.)*'")]
    #[doc(hidden)]
    StringLiteral,

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

    #[regex(r"[a-zA-Z][a-zA-Z0-9_.\-]*")]
    Id,

    #[token(".!")]
    DotBang,

    #[token(".")]
    Dot,

    #[regex(r"@[a-zA-Z][a-zA-Z0-9_]*")]
    CaptureToken,

    #[regex(r"@_[a-zA-Z0-9_]*")]
    DiscardToken,

    #[token("@")]
    At,

    #[regex(r"[ \t]+")]
    Whitespace,

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    #[regex(r"//[^\n]*", allow_greedy = true)]
    #[regex(r";[^\n]*", allow_greedy = true)]
    #[regex(r"#[^\n]*", allow_greedy = true)]
    LineComment,

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment,

    #[token("==")]
    OpEq,

    #[token("!=")]
    OpNe,

    #[token("^=")]
    OpStartsWith,

    #[token("$=")]
    OpEndsWith,

    #[token("*=")]
    OpContains,

    #[token("=~")]
    OpRegexMatch,

    #[token("!~")]
    OpRegexNoMatch,

    RegexLiteral,

    Garbage,
    Error,

    Root,
    ExpressionRoot,
    NamedNode,
    DefRef,
    Str,
    Field,
    CapturedPattern,
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
    NodePredicate,
    Regex,

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
        matches!(self, Error | Garbage)
    }
}

impl From<SyntaxKind> for rowan::SyntaxKind {
    #[inline]
    fn from(kind: SyntaxKind) -> Self {
        Self(kind as u16)
    }
}

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
