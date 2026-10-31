use logos::Logos;
use rowan::TextRange;
use std::ops::Range;

use super::cst::SyntaxKind;

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

struct Lexeme<'q> {
    text: &'q str,
    span: Range<usize>,
}

impl<'q> Lexeme<'q> {
    fn new(source: &'q str, span: Range<usize>) -> Self {
        let text = &source[span.clone()];
        Self { text, span }
    }

    fn start(&self) -> usize {
        self.span.start
    }

    fn end(&self) -> usize {
        self.span.end
    }
}

pub fn lex(source: &str) -> Vec<Token> {
    let mut tokens = Vec::with_capacity(source.len() / 4 + 8);
    let mut lexer = SyntaxKind::lexer(source);
    let mut error_start: Option<usize> = None;
    let mut expects_regex = false;

    loop {
        let remaining = lexer.remainder();
        if expects_regex
            && remaining.starts_with('/')
            && !remaining.starts_with("/*")
            && !remaining.starts_with("//")
        {
            let start = source.len() - remaining.len();
            let len = regex_literal_len(remaining);
            lexer.bump(len);
            tokens.push(Token::new(
                SyntaxKind::RegexLiteral,
                range_to_text_range(start..start + len),
            ));
            expects_regex = false;
            continue;
        }

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
                if !kind.is_trivia() {
                    expects_regex =
                        matches!(kind, SyntaxKind::OpRegexMatch | SyntaxKind::OpRegexNoMatch);
                }
                match kind {
                    SyntaxKind::StringLiteral => {
                        split_string_literal(Lexeme::new(source, span), &mut tokens);
                    }
                    _ => {
                        tokens.push(Token::new(kind, range_to_text_range(span)));
                    }
                }
            }
            Some(Err(())) => {
                expects_regex = false;
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

fn split_string_literal(lexeme: Lexeme<'_>, tokens: &mut Vec<Token>) {
    let quote_char = lexeme
        .text
        .chars()
        .next()
        .expect("StringLiteral always begins with a quote char");
    let quote_kind = if quote_char == '"' {
        SyntaxKind::DoubleQuote
    } else {
        SyntaxKind::SingleQuote
    };

    let start = lexeme.start();
    let end = lexeme.end();

    tokens.push(Token::new(
        quote_kind,
        range_to_text_range(start..start + 1),
    ));

    if end - start > 2 {
        tokens.push(Token::new(
            SyntaxKind::StringContent,
            range_to_text_range(start + 1..end - 1),
        ));
    }

    tokens.push(Token::new(quote_kind, range_to_text_range(end - 1..end)));
}

fn regex_literal_len(text: &str) -> usize {
    let mut escaped = false;

    for (offset, ch) in text[1..].char_indices() {
        if ch == '\n' {
            return offset + 1;
        }

        if ch == '/' && !escaped {
            return offset + 2;
        }

        escaped = ch == '\\' && !escaped;
    }

    text.len()
}

#[inline]
pub fn token_text<'q>(source: &'q str, token: &Token) -> &'q str {
    &source[std::ops::Range::<usize>::from(token.span)]
}

#[cfg(test)]
pub(super) fn dump_tokens(source: &str) -> String {
    let mut out = String::new();
    for token in lex(source) {
        if !token.kind.is_trivia() {
            out.push_str(&format!(
                "{:?} {:?}\n",
                token.kind,
                token_text(source, &token)
            ));
        }
    }
    out
}
