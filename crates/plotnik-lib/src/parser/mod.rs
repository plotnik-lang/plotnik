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
    Alt, AltKind, Anchor, Branch, Capture, Def, Expr, Field, NegatedField, Quantifier, Ref, Root,
    Seq, Str, Tree, Type, Wildcard,
};

pub use core::Parser;

use crate::PassResult;
use lexer::lex;

/// Parse result containing the green tree.
///
/// The tree is always complete—diagnostics are returned separately.
/// Error nodes in the tree represent recovery points.
#[derive(Debug, Clone)]
pub struct Parse {
    cst: rowan::GreenNode,
}

impl Parse {
    pub fn as_cst(&self) -> &rowan::GreenNode {
        &self.cst
    }

    /// Creates a typed view over the immutable green tree.
    /// This is cheap—SyntaxNode is a thin wrapper with parent pointers.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.cst.clone())
    }
}

/// Main entry point. Returns Err on fuel exhaustion.
pub fn parse(source: &str) -> PassResult<Parse> {
    parse_with_parser(Parser::new(source, lex(source)))
}

/// Parse with a pre-configured parser (for custom fuel limits).
pub(crate) fn parse_with_parser(mut parser: Parser) -> PassResult<Parse> {
    parser.parse_root();
    let (cst, diagnostics) = parser.finish()?;
    Ok((Parse { cst }, diagnostics))
}
