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
    pub(crate) kind: SyntaxKind,
    pub(crate) span: TextRange,
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
/// - Splits `RegexPredicateMatch`/`RegexPredicateNoMatch` into operator + whitespace + regex
pub fn lex(source: &str) -> Vec<Token> {
    // Every token spans at least one byte, so source.len() bounds the count;
    // the divisor approximates the average bytes per token.
    let mut tokens = Vec::with_capacity(source.len() / 4 + 8);
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
                match kind {
                    // A shebang is only meaningful on line 1; elsewhere `#!` is garbage.
                    SyntaxKind::Shebang if span.start != 0 => {
                        tokens.push(Token::new(SyntaxKind::Garbage, range_to_text_range(span)));
                    }
                    SyntaxKind::StringLiteral => {
                        split_string_literal(source, span, &mut tokens);
                    }
                    SyntaxKind::RegexPredicateMatch => {
                        split_regex_predicate(source, span, SyntaxKind::OpRegexMatch, &mut tokens);
                    }
                    SyntaxKind::RegexPredicateNoMatch => {
                        split_regex_predicate(
                            source,
                            span,
                            SyntaxKind::OpRegexNoMatch,
                            &mut tokens,
                        );
                    }
                    _ => {
                        tokens.push(Token::new(kind, range_to_text_range(span)));
                    }
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
    let text = &source[span.start..span.end];
    let quote_char = text
        .chars()
        .next()
        .expect("StringLiteral always begins with a quote char");
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

/// Splits a regex predicate token into: operator + whitespace (if any) + regex literal
///
/// Input: `=~ /pattern/` or `!~ /pattern/`
/// Output: `OpRegexMatch`/`OpRegexNoMatch` + `Whitespace`? + `RegexLiteral`
fn split_regex_predicate(
    source: &str,
    span: Range<usize>,
    op_kind: SyntaxKind,
    tokens: &mut Vec<Token>,
) {
    let start = span.start;
    let end = span.end;
    let text = &source[start..end];

    tokens.push(Token::new(op_kind, range_to_text_range(start..start + 2)));

    let regex_start_in_text = text[2..]
        .find('/')
        .expect("regex predicate always contains '/' after the 2-char operator")
        + 2;

    if regex_start_in_text > 2 {
        tokens.push(Token::new(
            SyntaxKind::Whitespace,
            range_to_text_range(start + 2..start + regex_start_in_text),
        ));
    }

    tokens.push(Token::new(
        SyntaxKind::RegexLiteral,
        range_to_text_range(start + regex_start_in_text..end),
    ));
}

/// Retrieves the text slice for a token. O(1) slice into source.
#[inline]
pub fn token_text<'q>(source: &'q str, token: &Token) -> &'q str {
    &source[std::ops::Range::<usize>::from(token.span)]
}

/// Render the non-trivia token stream as one `Kind "text"` line per token.
///
/// `Token.kind` is `pub(crate)`, so a downstream test cannot format the stream itself.
pub fn dump_tokens(source: &str) -> String {
    let mut out = String::new();
    for token in lex(source) {
        if !token.kind.is_trivia() {
            out.push_str(&format!("{:?} {:?}\n", token.kind, token_text(source, &token)));
        }
    }
    out
}
