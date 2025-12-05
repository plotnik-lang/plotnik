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
//! The parser is resilient—it always produces a tree. Recovery follows these rules:
//!
//! 1. Unknown tokens get wrapped in `SyntaxKind::Error` nodes and consumed
//! 2. Missing expected tokens emit a diagnostic but don't consume (parent may handle)
//! 3. Recovery sets define "synchronization points" per production
//! 4. On recursion limit, remaining input goes into single Error node
//!
//! However, fuel exhaustion (exec_fuel, recursion_fuel) returns an actual error immediately.

pub mod ast;
pub mod cst;
pub mod lexer;

mod core;
mod grammar;
mod invariants;

#[cfg(test)]
mod ast_tests;
#[cfg(test)]
mod cst_tests;
#[cfg(test)]
mod lexer_tests;
#[cfg(test)]
mod tests;

// Re-exports from cst (was syntax_kind)
pub use cst::{SyntaxKind, SyntaxNode, SyntaxToken};

// Re-exports from ast (was nodes)
pub use ast::{
    Alt, AltKind, Anchor, AnonymousNode, Branch, CapturedExpr, Def, Expr, FieldExpr, NamedNode,
    NegatedField, QuantifiedExpr, Ref, Root, Seq, Type,
};

pub use core::Parser;

use crate::PassResult;
use lexer::lex;

/// Main entry point. Returns Err on fuel exhaustion.
pub fn parse(source: &str) -> PassResult<Root> {
    parse_with_parser(Parser::new(source, lex(source)))
}

/// Parse with a pre-configured parser (for custom fuel limits).
pub(crate) fn parse_with_parser(mut parser: Parser) -> PassResult<Root> {
    parser.parse_root();
    let (cst, diagnostics) = parser.finish()?;
    let root = Root::cast(SyntaxNode::new_root(cst)).expect("parser always produces Root");
    Ok((root, diagnostics))
}
