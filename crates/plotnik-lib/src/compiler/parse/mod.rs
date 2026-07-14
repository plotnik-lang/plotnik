//! Parser infrastructure for the query language.
//!
//! This parser produces a lossless concrete syntax tree (CST) via Rowan's green tree builder.
//! Key design decisions borrowed from rust-analyzer, rnix-parser, and taplo:
//!
//! - Zero-copy parsing: tokens carry spans, text sliced only when building tree nodes
//! - Trivia buffering: whitespace/comments collected, then attached as leading trivia
//! - Checkpoint-based wrapping: retroactively wrap nodes for quantifiers `*+?`
//! - Explicit recovery sets: per-production sets determine when to bail vs consume diagnostics
//!
//! # Recovery Strategy
//!
//! The parser is resilient — it always produces a tree. Recovery follows these rules:
//!
//! 1. Unknown tokens get wrapped in `SyntaxKind::Error` nodes and consumed
//! 2. Missing expected tokens emit a diagnostic but don't consume (parent may handle)
//! 3. Recovery sets define "synchronization points" per production
//! 4. On recursion limit, remaining input goes into single Error node
//!
//! However, fuel exhaustion (parse_fuel, recursion_fuel) returns an actual error immediately.

pub use crate::compiler::diagnostics::Error;

pub(crate) mod ast;
pub(crate) mod cst;
mod lexer;
pub(crate) mod strings;
mod token_set;

mod grammar;
mod invariants;
mod parser;

#[cfg(test)]
mod ast_tests;
#[cfg(test)]
mod cst_tests;
#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod parser_tests;
#[cfg(test)]
mod strings_tests;
#[cfg(test)]
mod token_set_tests;
#[cfg(test)]
mod tokenize_tests;

pub use cst::{SyntaxKind, SyntaxNode};

pub use ast::{Alternative, Anchor, Def, NegatedField, Pattern, Root};

pub use parser::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, Parser};

pub use lexer::lex;

/// Parse one lossless query CST with caller-owned diagnostics and resource limits.
///
/// Syntax diagnostics remain in `diagnostics`; only fatal parser resource failures
/// are returned. Callers decide whether recovery trees are acceptable at their
/// boundary.
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
        SyntaxKind::LineComment | SyntaxKind::BlockComment | SyntaxKind::Shebang => "comment",
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
        | SyntaxKind::Tilde
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
