//! Lexer for the query language.
//!
//! Produces span-based tokens without storing text - text is sliced from source only when needed.
//!
//! ## Error handling
//!
//! The lexer coalesces consecutive error characters into single `Garbage` tokens rather
//! than producing one error per character. This keeps the token stream manageable for malformed input.

use logos::Logos;
use rowan::TextRange;
use std::ops::Range;

use super::cst::SyntaxKind;

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
/// Post-processes the Logos output:
/// - Coalesces consecutive lexer errors into single `Garbage` tokens
/// - Splits `StringLiteral` tokens into quote + content + quote
pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut lexer = SyntaxKind::lexer(source);
    let mut error_start: Option<usize> = None;

    loop {
        match lexer.next() {
            Some(Ok(kind)) => {
                if let Some(start) = error_start.take() {
                    let end = lexer.span().start;
                    tokens.push(Token::new(
                        SyntaxKind::Garbage,
                        range_to_text_range(start..end),
                    ));
                }

                let span = lexer.span();
                if kind == SyntaxKind::StringLiteral {
                    split_string_literal(source, span, &mut tokens);
                } else {
                    tokens.push(Token::new(kind, range_to_text_range(span)));
                }
            }
            Some(Err(())) => {
                if error_start.is_none() {
                    error_start = Some(lexer.span().start);
                }
            }
            None => {
                if let Some(start) = error_start.take() {
                    tokens.push(Token::new(
                        SyntaxKind::Garbage,
                        range_to_text_range(start..source.len()),
                    ));
                }
                break;
            }
        }
    }

    tokens
}

/// Splits a string literal token into: quote + content + quote
fn split_string_literal(source: &str, span: Range<usize>, tokens: &mut Vec<Token>) {
    let text = &source[span.clone()];
    let quote_char = text.chars().next().unwrap();
    let quote_kind = if quote_char == '"' {
        SyntaxKind::DoubleQuote
    } else {
        SyntaxKind::SingleQuote
    };

    let start = span.start;
    let end = span.end;

    tokens.push(Token::new(
        quote_kind,
        range_to_text_range(start..start + 1),
    ));

    if end - start > 2 {
        tokens.push(Token::new(
            SyntaxKind::StrVal,
            range_to_text_range(start + 1..end - 1),
        ));
    }

    tokens.push(Token::new(quote_kind, range_to_text_range(end - 1..end)));
}

/// Retrieves the text slice for a token. O(1) slice into source.
#[inline]
pub fn token_text<'q>(source: &'q str, token: &Token) -> &'q str {
    &source[std::ops::Range::<usize>::from(token.span)]
}
