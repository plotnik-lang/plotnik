//! Parser infrastructure for the query language.
//!
//! # Architecture
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

pub mod ast;
mod cst;
mod lexer;

mod core;
mod grammar;
mod invariants;

#[cfg(test)]
mod ast_tests;
#[cfg(test)]
mod cst_tests;
#[cfg(test)]
mod tests;

pub use cst::{SyntaxKind, SyntaxNode, SyntaxToken};

pub use ast::{
    AltKind, Anchor, Branch, CapturedPattern, Def, EnumPattern, Pattern, FieldPattern, TokenPattern,
    NegatedField, NodePattern, NodePredicate, PredicateOp, PredicateValue, QuantifiedPattern, Ref,
    RegexLiteral, Root, SeqPattern, SeqItem, Type, UnionPattern, is_empty_group, token_src,
};
pub(crate) use ast::classify_alt;

pub use core::{DEFAULT_FUEL, DEFAULT_MAX_DEPTH, ParseConfig, ParseResult, Parser};

pub use lexer::{Token, dump_tokens, lex, token_text};
