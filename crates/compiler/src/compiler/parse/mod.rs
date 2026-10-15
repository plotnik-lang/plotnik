pub use crate::compiler::diagnostics::Error;

pub(crate) mod ast;
pub(crate) mod cst;
mod lexer;
pub(crate) mod strings;
mod token_set;

mod grammar;
mod parser;

#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod parser_tests;

pub use cst::{SyntaxKind, SyntaxNode};

pub use ast::{Alternative, Anchor, Def, NegatedField, Pattern, Root};

pub use parser::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, Parser};

pub use lexer::lex;

/// The syntax role of a source supplied without an explicit role.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputKind {
    Module,
    Expression,
}

/// Parse unclassified input using the ordinary parser limits.
///
/// Named definitions and empty modules select module parsing. Otherwise the
/// expression parser checks the input. Syntax diagnostics are left to the
/// caller's normal compilation path. Fatal resource failures are returned.
pub fn parse_input_kind(source: crate::compiler::Source<'_>) -> Result<InputKind, Error> {
    let mut diagnostics = crate::compiler::Diagnostics::new();
    let root = parse_lossless(
        source.content,
        source.id,
        &mut diagnostics,
        ParseConfig::default(),
    )?;
    if root.defs().next().is_none() || root.defs().any(|definition| definition.name().is_some()) {
        return Ok(InputKind::Module);
    }

    Parser::new(
        source.content,
        source.id,
        lex(source.content),
        &mut crate::compiler::Diagnostics::new(),
        ParseConfig::default(),
    )
    .parse_expression()?;
    Ok(InputKind::Expression)
}

pub(crate) fn parse_lossless(
    source: &str,
    source_id: crate::compiler::diagnostics::SourceId,
    diagnostics: &mut crate::compiler::diagnostics::Diagnostics,
    config: ParseConfig,
) -> Result<Root, Error> {
    Parser::new(source, source_id, lex(source), diagnostics, config)
        .parse()
        .map(parser::ParsedRoot::into_ast)
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct QueryToken {
    /// Stable lowercase token class for editor/highlighter consumers.
    pub kind: &'static str,
    /// Half-open byte span in the query text.
    pub span: (u32, u32),
}

/// Editor-grade tokenization from the real query lexer.
///
/// Tokenization is total: malformed input is represented with `error` spans.
pub fn tokenize(text: &str) -> Vec<QueryToken> {
    lex(text)
        .into_iter()
        .map(|token| {
            let range = std::ops::Range::<usize>::from(token.span);
            QueryToken {
                kind: class_name(token.kind),
                span: (
                    u32::try_from(range.start).expect("token start byte fits in u32"),
                    u32::try_from(range.end).expect("token end byte fits in u32"),
                ),
            }
        })
        .collect()
}

fn class_name(kind: SyntaxKind) -> &'static str {
    match kind {
        SyntaxKind::Whitespace | SyntaxKind::Newline => "whitespace",
        SyntaxKind::LineComment | SyntaxKind::BlockComment => "comment",
        SyntaxKind::DoubleQuote
        | SyntaxKind::SingleQuote
        | SyntaxKind::StringContent
        | SyntaxKind::StringLiteral
        | SyntaxKind::UnterminatedString => "string",
        SyntaxKind::RegexLiteral
        | SyntaxKind::RegexPredicateMatch
        | SyntaxKind::RegexPredicateNoMatch => "regex",
        SyntaxKind::CaptureToken | SyntaxKind::DiscardToken | SyntaxKind::At => "capture",
        SyntaxKind::Id | SyntaxKind::KwError | SyntaxKind::KwMissing => "ident",
        SyntaxKind::Garbage | SyntaxKind::Error => "error",
        SyntaxKind::ParenOpen
        | SyntaxKind::ParenClose
        | SyntaxKind::BracketOpen
        | SyntaxKind::BracketClose
        | SyntaxKind::BraceOpen
        | SyntaxKind::BraceClose
        | SyntaxKind::DoubleColon
        | SyntaxKind::Colon
        | SyntaxKind::Equals
        | SyntaxKind::Negation
        | SyntaxKind::Minus
        | SyntaxKind::Underscore
        | SyntaxKind::Star
        | SyntaxKind::Plus
        | SyntaxKind::Question
        | SyntaxKind::StarQuestion
        | SyntaxKind::PlusQuestion
        | SyntaxKind::QuestionQuestion
        | SyntaxKind::Slash
        | SyntaxKind::Hash
        | SyntaxKind::Comma
        | SyntaxKind::Pipe
        | SyntaxKind::DotBang
        | SyntaxKind::Dot
        | SyntaxKind::OpEq
        | SyntaxKind::OpNe
        | SyntaxKind::OpStartsWith
        | SyntaxKind::OpEndsWith
        | SyntaxKind::OpContains
        | SyntaxKind::OpRegexMatch
        | SyntaxKind::OpRegexNoMatch => "punct",
        _ => "error",
    }
}
