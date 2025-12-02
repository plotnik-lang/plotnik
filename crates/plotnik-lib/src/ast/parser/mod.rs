//! Resilient LL parser for the query language.
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
//! The parser never fails—it always produces a tree. Recovery follows these rules:
//!
//! 1. Unknown tokens get wrapped in `SyntaxKind::Error` nodes and consumed
//! 2. Missing expected tokens emit an error but don't consume (parent may handle)
//! 3. Recovery sets define "synchronization points" per production
//! 4. On recursion limit, remaining input goes into single Error node
//!
//! # Grammar (EBNF-ish)
//!
//! ```text
//! root       = expr*
//! expr       = tree | alternation | wildcard | anon_node
//!            | anchor | negated_field | field | ident
//! tree       = "(" [node_type] expr* ")"
//! alternation= "[" expr* "]"
//! wildcard   = "_"
//! anon_node  = STRING
//! capture    = "@" LOWER_IDENT
//! anchor     = "."
//! negated_field = "!" IDENT
//! field      = IDENT ":" expr
//! quantifier = expr ("*" | "+" | "?" | "*?" | "+?" | "??")
//! capture    = expr "@" IDENT ["::" TYPE]
//! ```

mod core;
mod error;
mod grammar;

pub use error::{
    Diagnostic, ErrorStage, Fix, RelatedInfo, RenderOptions, Severity, SyntaxError,
    render_diagnostics, render_errors,
};

pub use core::ParserOptions;
use core::{Parse as ParseInner, Parser};

use super::lexer::lex;
use super::syntax_kind::SyntaxNode;

/// Stack depth limit. Tree-sitter queries can nest deeply via `(a (b (c ...)))`.
/// 512 handles any reasonable input while preventing stack overflow on malicious input.
const MAX_DEPTH: u32 = 512;

/// Parse result containing the green tree and any errors.
///
/// The tree is always complete—errors are recorded separately and also
/// represented as `SyntaxKind::Error` nodes in the tree itself.
#[derive(Debug, Clone)]
pub struct Parse {
    inner: ParseInner,
}

impl Parse {
    #[allow(dead_code)]
    pub fn green(&self) -> &rowan::GreenNode {
        &self.inner.green
    }

    /// Creates a typed view over the immutable green tree.
    /// This is cheap—SyntaxNode is a thin wrapper with parent pointers.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.inner.green.clone())
    }

    pub fn errors(&self) -> &[SyntaxError] {
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

/// Main entry point. Always succeeds—errors embedded in the returned tree.
pub fn parse(source: &str) -> Parse {
    parse_with_options(source, ParserOptions::default())
}

/// Parse with custom options (e.g., disable fuel for pathological test inputs).
#[cfg(debug_assertions)]
pub fn parse_with_options(source: &str, options: ParserOptions) -> Parse {
    let tokens = lex(source);
    let mut parser = Parser::with_options(source, tokens, options);
    parser.parse_root();
    Parse {
        inner: parser.finish(),
    }
}

#[cfg(not(debug_assertions))]
pub fn parse_with_options(source: &str, _options: ParserOptions) -> Parse {
    let tokens = lex(source);
    let mut parser = Parser::with_options(source, tokens, ParserOptions::default());
    parser.parse_root();
    Parse {
        inner: parser.finish(),
    }
}

#[cfg(test)]
mod tests;
