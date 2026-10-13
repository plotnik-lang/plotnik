use logos::Logos;
use rowan::TextRange;

use crate::compiler::diagnostics::{Source, Span};
use crate::compiler::parse::SyntaxKind;

/// A language name or repository path with an optional Git revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanguageSelector {
    pub name: String,
    pub revision: Option<String>,
}

impl LanguageSelector {
    /// Parse exactly the selector accepted by CLI `--lang`, without trimming.
    pub fn parse(input: &str) -> Result<Self, &'static str> {
        let (name, revision) = match input.split_once('@') {
            Some((name, revision)) => {
                if revision.is_empty()
                    || revision.contains('@')
                    || revision.chars().any(char::is_whitespace)
                    || revision.chars().any(char::is_control)
                {
                    return Err("expected language@revision with a nonempty revision");
                }

                let revision =
                    if revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()) {
                        revision.to_ascii_lowercase()
                    } else {
                        revision.to_owned()
                    };
                (name, Some(revision))
            }
            None => (input, None),
        };
        if name.split('/').any(|part| {
            part.is_empty()
                || matches!(part, "." | "..")
                || part.contains('\\')
                || part.chars().any(char::is_control)
        }) {
            return Err(
                "expected language name or owner/repo[/grammar-directory] without empty or traversal components",
            );
        }

        Ok(Self {
            name: name.to_owned(),
            revision,
        })
    }
}

/// A first-line declaration. Raw spelling and spans exclude the line terminator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarDeclaration<'q> {
    pub raw: &'q str,
    pub selector: LanguageSelector,
    pub span: Span,
    pub selector_span: Span,
}

impl<'q> GrammarDeclaration<'q> {
    pub fn raw_selector(&self) -> &'q str {
        let start = usize::from(self.selector_span.range.start() - self.span.range.start());
        let end = usize::from(self.selector_span.range.end() - self.span.range.start());
        &self.raw[start..end]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrammarDeclarationError {
    pub message: &'static str,
    pub span: Span,
}

/// Read a declaration without resolving a grammar or analyzing definitions.
pub fn parse_grammar_declaration(
    source: Source<'_>,
) -> Result<Option<GrammarDeclaration<'_>>, GrammarDeclarationError> {
    let line = match source.content.split_once('\n') {
        Some((line, _)) => line.strip_suffix('\r').unwrap_or(line),
        None => source.content,
    };
    // Reuse the comment lexer, including its block-comment rules. Nothing is
    // removed from query syntax, and a second item makes this ordinary trivia.
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
    let Some(selector) = payload.strip_prefix('!') else {
        return Ok(None);
    };
    let raw_selector = selector.trim_matches([' ', '\t']);

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
    let selector_start = raw_selector.as_ptr() as usize - line.as_ptr() as usize;
    let span = span_at(comment.start, comment.end);
    let selector_span = span_at(selector_start, selector_start + raw_selector.len());
    let selector =
        LanguageSelector::parse(raw_selector).map_err(|message| GrammarDeclarationError {
            message,
            span: selector_span,
        })?;

    Ok(Some(GrammarDeclaration {
        raw,
        selector,
        span,
        selector_span,
    }))
}
