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
//! - Explicit recovery sets: per-production sets determine when to bail vs consume errors
//!
//! # Error Recovery Strategy
//!
//! The parser never fails on syntax errors—it always produces a tree. Recovery follows these rules:
//!
//! 1. Unknown tokens get wrapped in `SyntaxKind::Error` nodes and consumed
//! 2. Missing expected tokens emit an error but don't consume (parent may handle)
//! 3. Recovery sets define "synchronization points" per production
//! 4. On recursion limit, remaining input goes into single Error node
//!
//! However, fuel exhaustion (exec_fuel, recursion_fuel) returns an error immediately.

pub mod ast;
pub mod cst;
pub mod lexer;

mod core;
mod error;
mod grammar;
mod invariants;

#[cfg(test)]
mod ast_tests;
#[cfg(test)]
mod cst_tests;
#[cfg(test)]
mod error_tests;
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

// Re-exports from error
pub use error::{
    Diagnostic, ErrorStage, Fix, RelatedInfo, RenderOptions, Severity, SyntaxError,
    render_diagnostics, render_errors,
};

// Internal use
pub(crate) use core::Parser;

use crate::Result;
use lexer::lex;

/// Parse result containing the green tree and any errors.
///
/// The tree is always complete—errors are recorded separately and also
/// represented as `SyntaxKind::Error` nodes in the tree itself.
#[derive(Debug, Clone)]
pub struct Parse {
    inner: core::Parse,
}

impl Parse {
    #[allow(dead_code)]
    pub fn as_cst(&self) -> &rowan::GreenNode {
        &self.inner.cst
    }

    /// Creates a typed view over the immutable green tree.
    /// This is cheap—SyntaxNode is a thin wrapper with parent pointers.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.inner.cst.clone())
    }

    pub fn errors(&self) -> &[Diagnostic] {
        &self.inner.errors
    }

    #[allow(dead_code)]
    pub fn is_valid(&self) -> bool {
        self.inner.errors.is_empty()
    }

    /// Render errors as a human-readable string using annotate-snippets.
    pub fn render_errors(&self, source: &str) -> String {
        render_errors(source, &self.inner.errors, None)
    }
}

/// Main entry point. Returns Err on fuel exhaustion.
pub fn parse(source: &str) -> Result<Parse> {
    parse_with_parser(Parser::new(source, lex(source)))
}

/// Parse with a pre-configured parser (for custom fuel limits).
pub(crate) fn parse_with_parser(mut parser: Parser) -> Result<Parse> {
    parser.parse_root();
    Ok(Parse {
        inner: parser.finish()?,
    })
}
