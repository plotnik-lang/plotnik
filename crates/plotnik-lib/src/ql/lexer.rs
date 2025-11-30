//! Lexer for the query language.
//!
//! Produces span-based tokens without storing text - text is sliced from source only when needed.
//!
//! ## Error handling
//!
//! The lexer coalesces consecutive error characters into single `UnexpectedFragment` tokens rather
//! than producing one error per character. This keeps the token stream manageable for malformed input.

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

fn range_to_text_range(range: Range<usize>) -> TextRange {
    TextRange::new((range.start as u32).into(), (range.end as u32).into())
}

/// Tokenizes source into a vector of span-based tokens.
///
/// Post-processes the Logos output to coalesce consecutive lexer errors into single `UnexpectedFragment` tokens.
pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut lexer = SyntaxKind::lexer(source);
    let mut error_start: Option<usize> = None;

    loop {
        match lexer.next() {
            Some(Ok(kind)) => {
                // Flush accumulated error span before emitting valid token
                if let Some(start) = error_start.take() {
                    let end = lexer.span().start;
                    tokens.push(Token::new(
                        SyntaxKind::UnexpectedFragment,
                        range_to_text_range(start..end),
                    ));
                }

                let span = lexer.span();
                tokens.push(Token::new(kind, range_to_text_range(span)));
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
