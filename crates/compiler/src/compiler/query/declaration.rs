use logos::Logos;
use rowan::TextRange;

use crate::compiler::diagnostics::{Source, Span};
use crate::compiler::parse::SyntaxKind;

/// A first-line declaration. Raw spelling and spans exclude the line terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarDeclaration<'q> {
    pub raw: &'q str,
    pub language: &'q str,
    pub span: Span,
    pub language_span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarDeclarationError {
    pub message: &'static str,
    pub span: Span,
}

/// Read and validate a language name from a first-line comment.
pub fn parse_grammar_declaration(
    source: Source<'_>,
) -> Result<Option<GrammarDeclaration<'_>>, GrammarDeclarationError> {
    let line = match source.content.split_once('\n') {
        Some((line, _)) => line.strip_suffix('\r').unwrap_or(line),
        None => source.content,
    };
    // The comment lexer determines eligibility, including block-comment bounds.
    // A declaration occupies the first physical line's sole comment.
    let mut lexer = SyntaxKind::lexer(line);
    let mut kind = lexer.next();
    if kind == Some(Ok(SyntaxKind::Whitespace)) {
        kind = lexer.next();
    }
    let comment = lexer.span();
    let padding = match kind {
        Some(Ok(SyntaxKind::LineComment)) => {
            if line[comment.clone()].starts_with("//") {
                2
            } else {
                1
            }
        }
        Some(Ok(SyntaxKind::BlockComment)) => 2,
        _ => return Ok(None),
    };
    match lexer.next() {
        None => {}
        Some(Ok(SyntaxKind::Whitespace)) if lexer.next().is_none() => {}
        _ => return Ok(None),
    }

    let raw = &line[comment.clone()];
    let payload = if kind == Some(Ok(SyntaxKind::BlockComment)) {
        &raw[padding..raw.len() - 2]
    } else {
        &raw[padding..]
    };
    let payload = payload.trim_matches([' ', '\t']);
    let Some(language) = payload.strip_prefix('@') else {
        return Ok(None);
    };
    let language = language.trim_matches([' ', '\t']);

    let span_at = |start: usize, end: usize| {
        Span::new(
            source.id,
            TextRange::new(
                u32::try_from(start)
                    .expect("query byte offsets fit in u32")
                    .into(),
                u32::try_from(end)
                    .expect("query byte offsets fit in u32")
                    .into(),
            ),
        )
    };
    let language_start = language.as_ptr() as usize - line.as_ptr() as usize;
    let span = span_at(comment.start, comment.end);
    let language_span = span_at(language_start, language_start + language.len());
    if !language.starts_with(|c: char| c.is_ascii_alphabetic())
        || !language
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
    {
        return Err(GrammarDeclarationError {
            message: "expected a language name: a letter followed by letters, digits, '-' or '_'",
            span: language_span,
        });
    }

    Ok(Some(GrammarDeclaration {
        raw,
        language,
        span,
        language_span,
    }))
}
