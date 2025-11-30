//! Lexer for the query language.
//!
//! Produces span-based tokens without storing text - text is sliced from source only when needed.
//!
//! ## Error handling
//!
//! The lexer coalesces consecutive error characters into single `Error` tokens rather than
//! producing one error per character. This keeps the token stream manageable for malformed input.

use logos::Logos;
use rowan::TextRange;
use std::ops::Range;

use super::syntax_kind::SyntaxKind;

/// Zero-copy token: kind + span, text retrieved via [`token_text`] when needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub kind: SyntaxKind,
    pub span: TextRange,
}

impl Token {
    #[inline]
    pub fn new(kind: SyntaxKind, span: TextRange) -> Self {
        Self { kind, span }
    }
}

/// Internal Logos token enum.
///
/// Converted to [`SyntaxKind`] after lexing. Separate enum because Logos derives
/// its lexer from enum variants.
#[derive(Logos, Debug, PartialEq, Clone)]
enum LexToken {
    #[token("(")]
    ParenOpen,

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

    #[token(".")]
    Dot,

    #[token("@")]
    At,

    // Non-greedy quantifiers must be listed before greedy ones for longest-match priority.
    #[token("*?")]
    StarQuestion,

    #[token("+?")]
    PlusQuestion,

    #[token("??")]
    QuestionQuestion,

    #[token("*")]
    Star,

    #[token("+")]
    Plus,

    #[token("?")]
    Question,

    /// Double-quoted string with backslash escapes.
    #[regex(r#""(?:[^"\\]|\\.)*""#)]
    String,

    /// PascalCase identifier (e.g., `FunctionDeclaration`). Used for supertype patterns.
    #[regex(r"[A-Z][A-Za-z0-9]*")]
    UpperIdentifier,

    /// snake_case identifier (e.g., `function_definition`). Standard node/field names and captures.
    #[regex(r"[a-z][a-z0-9_]*")]
    LowerIdentifier,

    #[regex(r"/\*(?:[^*]|\*[^/])*\*/")]
    BlockComment,

    #[regex(r"//[^\n]*")]
    LineComment,

    /// Horizontal whitespace only (spaces, tabs). Newlines tracked separately for
    /// potential future line-aware error reporting.
    #[regex(r"[ \t]+")]
    Whitespace,

    #[token("\n")]
    #[token("\r\n")]
    Newline,

    /// XML-like tags are explicitly matched and marked as errors.
    /// Common mistake of LLM agents while generating code.
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*(?:\s+[^>]*)?>")]
    #[regex(r"</[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*>")]
    #[regex(r"<[a-zA-Z_:][a-zA-Z0-9_:\.\-]*\s*/\s*>")]
    UnexpectedXML,
}

impl LexToken {
    fn to_syntax_kind(&self) -> SyntaxKind {
        match self {
            LexToken::ParenOpen => SyntaxKind::ParenOpen,
            LexToken::ParenClose => SyntaxKind::ParenClose,
            LexToken::BracketOpen => SyntaxKind::BracketOpen,
            LexToken::BracketClose => SyntaxKind::BracketClose,
            LexToken::Colon => SyntaxKind::Colon,
            LexToken::Equals => SyntaxKind::Equals,
            LexToken::Negation => SyntaxKind::Negation,
            LexToken::Tilde => SyntaxKind::Tilde,
            LexToken::Underscore => SyntaxKind::Underscore,
            LexToken::Dot => SyntaxKind::Dot,
            LexToken::At => SyntaxKind::At,
            LexToken::Star => SyntaxKind::Star,
            LexToken::Plus => SyntaxKind::Plus,
            LexToken::Question => SyntaxKind::Question,
            LexToken::StarQuestion => SyntaxKind::StarQuestion,
            LexToken::PlusQuestion => SyntaxKind::PlusQuestion,
            LexToken::QuestionQuestion => SyntaxKind::QuestionQuestion,
            LexToken::String => SyntaxKind::StringLit,
            LexToken::UpperIdentifier => SyntaxKind::UpperIdent,
            LexToken::LowerIdentifier => SyntaxKind::LowerIdent,
            LexToken::BlockComment => SyntaxKind::BlockComment,
            LexToken::LineComment => SyntaxKind::LineComment,
            LexToken::Whitespace => SyntaxKind::Whitespace,
            LexToken::Newline => SyntaxKind::Newline,
            LexToken::UnexpectedXML => SyntaxKind::UnexpectedXML,
        }
    }
}

fn range_to_text_range(range: Range<usize>) -> TextRange {
    TextRange::new((range.start as u32).into(), (range.end as u32).into())
}

/// Tokenizes source into a vector of span-based tokens.
///
/// Post-processes the Logos output to coalesce consecutive lexer errors into single `Error` tokens.
pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut lexer = LexToken::lexer(source);
    let mut error_start: Option<usize> = None;

    loop {
        match lexer.next() {
            Some(Ok(lex_token)) => {
                // Flush accumulated error span before emitting valid token
                if let Some(start) = error_start.take() {
                    let end = lexer.span().start;
                    tokens.push(Token::new(SyntaxKind::UnexpectedFragment, range_to_text_range(start..end)));
                }

                let span = lexer.span();
                tokens.push(Token::new(lex_token.to_syntax_kind(), range_to_text_range(span)));
            }
            Some(Err(())) => {
                // Accumulate error span; will be flushed on next valid token or EOF
                if error_start.is_none() {
                    error_start = Some(lexer.span().start);
                }
            }
            None => {
                if let Some(start) = error_start.take() {
                    tokens.push(Token::new(
                        SyntaxKind::UnexpectedFragment,
                        range_to_text_range(start..source.len()),
                    ));
                }
                break;
            }
        }
    }

    tokens
}

/// Retrieves the text slice for a token. O(1) slice into source.
#[inline]
pub fn token_text<'src>(source: &'src str, token: &Token) -> &'src str {
    &source[std::ops::Range::<usize>::from(token.span)]
}