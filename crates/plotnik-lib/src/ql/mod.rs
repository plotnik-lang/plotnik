//! Query language lexer, parser, and syntax types.
//!
//! This module provides a resilient parser for tree-sitter-like query syntax,
//! producing a lossless concrete syntax tree using [Rowan](https://docs.rs/rowan).
//!
//! # Architecture
//!
//! The parsing pipeline follows patterns from rust-analyzer and similar projects:
//!
//! ```text
//! Source text → Lexer → Tokens → Parser → GreenNode → SyntaxNode
//!                                              ↓
//!                                     Vec<SyntaxError>
//! ```
//!
//! - [`lexer`]: Logos-based tokenizer producing `Token { kind, span }` pairs.
//!   Tokens are zero-copy—text is sliced from source only when building the tree.
//!
//! - [`parser`]: Resilient LL parser using Rowan's `GreenNodeBuilder`. Key features:
//!   - Trivia buffering: whitespace/comments attach as leading trivia to nodes
//!   - Checkpoint API: enables retroactive node wrapping (e.g., quantifiers)
//!   - Recovery sets: per-production FOLLOW sets guide error recovery
//!   - Fuel mechanism (debug): detects infinite loops in lookahead
//!
//! - [`syntax_kind`]: `SyntaxKind` enum covering all tokens and nodes,
//!   plus `TokenSet` bitset for O(1) membership testing.
//!
//! # Error Handling
//!
//! The parser is designed to never fail outright. On invalid input:
//! 1. An error is recorded with span and message
//! 2. Unexpected tokens are wrapped in `Error` nodes
//! 3. Parsing continues at the nearest recovery point
//!
//! This ensures downstream tooling (CLI, LSP) always has a tree to work with.

pub mod lexer;
pub mod parser;
pub mod syntax_kind;

#[cfg(test)]
mod lexer_tests;
